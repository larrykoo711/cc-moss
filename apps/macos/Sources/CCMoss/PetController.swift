import AppKit
import MossKit
import Observation
import SwiftUI

/// Mo on the desktop: follows the same `AppModel` as the menu bar (no
/// refresh of its own), turns sessions into a pose, and runs the window,
/// the layers' loops and the few timers that start one-off moves.
///
/// Work happens only when something changes: a refresh, a timer for the
/// next blink or wave, the pointer entering Mo. Loops are layer animations
/// played by the render server, removed (not paused) when nobody can see them.
@MainActor
final class PetController {
    // Settings › General › Desktop Pet.
    static let enabledKey = "petEnabled"
    static let sizeKey = "petSize"
    static let placementKey = "petPlacement"
    static let tuckedKey = "petTucked"
    static let doNotDisturbKey = "petDoNotDisturbUntil"
    /// Posted by the settings card when the switch or the size changes.
    static let settingsChanged = Notification.Name("cc-moss.petSettingsChanged")
    static let sizes: [CGFloat] = [48, 64, 88]
    static let defaultSize: CGFloat = 64

    // Hooks for the message box (T3.3): the pointer on Mo, a click on Mo.
    var onHover: ((Bool) -> Void)?
    var onClick: (() -> Void)?
    /// An alert the router gave to Mo (A23): Mo, not Notification Center,
    /// tells you about it. T3.3 turns it into a bubble.
    var onAlert: ((SessionAlert) -> Void)?
    /// The alerts Mo holds, newest last, until T3.3 shows them.
    private(set) var alerts: [SessionAlert] = []
    /// Where Mo is on screen, to put a message box beside it.
    var frame: NSRect? { window?.frame }

    private let model: AppModel
    private let showPanel: () -> Void
    private let defaults: UserDefaults
    /// The bench runs Mo off screen and pretends it is visible.
    private let offScreen: Bool

    private var window: PetPanel?
    private var view: PetView?
    private var gate: PetMouseGate?
    private var size: CGFloat = 0
    private var stage = PetStage()
    private var presence = PetPresence()
    private var shown: PetPose?
    private var poseSince: TimeInterval = 0
    private var hovering = false
    private var occluded = false
    private var screensOff = false
    private var locked = false
    private var reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    private var timers: [String: Timer] = [:]
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []
    private var watching = 0
    private var openSettings: OpenWindowAction?
    private var placement = PetPlacement()
    private var tucked = false
    private var peeking: Bool?
    /// What Mo says and when (T3.3), and the bubble window it says it in.
    private var director = PetDirector(mode: .everyday)
    private var bubbles: PetBubbles?
    private var onBubble = false
    /// The right-click bubble is open, and what closes it on a click elsewhere.
    private var menuOpen = false
    private var menuMonitors: [Any] = []
    /// Mo's voice (T3.4): off, a chime or a spoken title, by Settings › Mo › Voice.
    private let voice: VoiceOutput
    private var announcer = VoiceAnnouncer()

    init(model: AppModel, showPanel: @escaping () -> Void, defaults: UserDefaults = .standard, offScreen: Bool = false,
         voice: VoiceOutput? = nil) {
        self.model = model
        self.showPanel = showPanel
        self.defaults = defaults
        self.offScreen = offScreen
        self.voice = voice ?? Speaker.shared
        NotificationCenter.default.addObserver(forName: Self.settingsChanged, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.sync() }
        }
        sync()
    }

    var isShown: Bool { window != nil }

    /// Mo is on screen where you can see it: switched on, not tucked away,
    /// not covered (a full-screen app), the screen awake and unlocked. The
    /// alert router gives Mo an alert only then (A23).
    var isVisibleToYou: Bool { window != nil && !offScreen && visible && !tucked }

    static func doNotDisturb(_ defaults: UserDefaults = .standard) -> Bool {
        defaults.double(forKey: doNotDisturbKey) > Date().timeIntervalSince1970
    }

    /// The router decided this one is Mo's to tell. Mo hints first (the
    /// antenna lamp, on top of the pose the snapshot gives); what it says
    /// depends on the interaction mode (DESIGN-pet §6.5).
    func take(_ alert: SessionAlert) {
        alerts.append(alert)
        if alerts.count > 20 { alerts.removeFirst(alerts.count - 20) }
        onAlert?(alert)
        guard window != nil else { return }
        director.doNotDisturb = doNotDisturbUntil != nil
        if director.take(alert, at: now) {
            view?.flashLamp()
            sound(alert.kind)
        }
        show()
    }

    /// Mo just hinted at something new: maybe say its title, or chime
    /// (DESIGN-pet §6.4). Only alerts routed to Mo get here; Notification
    /// Center makes its own sound. The bench never makes a sound.
    private func sound(_ kind: AlertKind) {
        guard !offScreen, let message = director.message else { return }
        announcer.announce(kind: kind, phrase: VoicePolicy.phrase(message, names: names(message)),
                           mode: director.mode, voice: VoiceMode(defaults: defaults),
                           doNotDisturb: doNotDisturbUntil != nil,
                           storedVoice: defaults.string(forKey: VoicePicker.key),
                           installed: { Speaker.installedVoices() }, at: now, to: voice)
    }

    static func enabled(_ defaults: UserDefaults = .standard) -> Bool { defaults.bool(forKey: enabledKey) }

    static func size(_ defaults: UserDefaults = .standard) -> CGFloat {
        let v = CGFloat(defaults.double(forKey: sizeKey))
        return sizes.contains(v) ? v : defaultSize
    }

    private var now: TimeInterval { ProcessInfo.processInfo.systemUptime }

    /// Show, hide or resize Mo to match the settings.
    func sync() {
        director.mode = PetInteraction(defaults: defaults)
        let want = Self.enabled(defaults)
        let wantSize = Self.size(defaults)
        if window != nil, !want || wantSize != size { teardown() }
        if want, window == nil { build(size: wantSize) }
        if window != nil { show() }
    }

    // MARK: - Window

    private func build(size: CGFloat) {
        self.size = size
        if let data = defaults.data(forKey: Self.placementKey), let saved = try? JSONDecoder().decode(PetPlacement.self, from: data) {
            placement = saved
        }
        tucked = defaults.bool(forKey: Self.tuckedKey)
        let side = PetView.side(body: size)
        let panel = PetPanel(side: side)
        let container = NSView(frame: NSRect(x: 0, y: 0, width: side, height: side))
        let pet = PetView(body: size)
        container.addSubview(pet)
        let bubbles = PetBubbles()
        bubbles.onPointer = { [weak self] inside in self?.pointedAtBubble(inside) }
        self.bubbles = bubbles
        let opener = NSHostingView(rootView: SettingsOpener { [weak self] in self?.openSettings = $0 })
        opener.frame = .zero
        container.addSubview(opener)
        panel.contentView = container
        window = panel
        view = pet
        wire(pet)
        if PetClickThrough.current == .toggle { gate = PetMouseGate(window: panel, view: pet) }
        peeking = nil
        place()
        panel.orderFrontRegardless()
        watchSystem(panel)
        shown = nil
        stage = PetStage()
        presence = PetPresence()
        update()
        watching += 1
        observeModel(watching)
    }

    private func teardown() {
        timers.values.forEach { $0.invalidate() }
        timers.removeAll()
        for (center, o) in observers { center.removeObserver(o) }
        observers.removeAll()
        gate?.stop()
        gate = nil
        view?.setLoops([])
        closeMenu(render: false)
        bubbles?.close()
        bubbles = nil
        onBubble = false
        window?.orderOut(nil)
        window = nil
        view = nil
        watching += 1 // stale observation callbacks stop re-registering
    }

    private func wire(_ pet: PetView) {
        pet.onClick = { [weak self] count in self?.clicked(count) }
        pet.onDragEnded = { [weak self] in self?.dropped() }
        pet.onRightClick = { [weak self] event in self?.menu(event) }
        pet.onHover = { [weak self] inside in self?.hovered(inside) }
        pet.onPointer = { [weak self] p in self?.view?.look(toward: p) }
    }

    /// The screens, with the one under the pointer as "main": Mo first
    /// appears where you are looking, not on a screen you may not use.
    static func screens() -> [PetScreen] {
        let mouse = NSEvent.mouseLocation
        let here = NSScreen.screens.first { $0.frame.contains(mouse) } ?? NSScreen.screens.first
        return NSScreen.screens.map { s in
            PetScreen(id: s.petID, frame: s.frame, visible: s.visibleFrame, isMain: s == here)
        }
    }

    /// Where Mo should be now: the remembered spot, half tucked away unless
    /// it wants you.
    private func place(animated: Bool = false) {
        guard let window else { return }
        if offScreen {
            window.setFrameOrigin(NSPoint(x: -20000, y: -20000))
            return
        }
        let screens = Self.screens()
        let side = window.frame.size
        var target = placement.frame(size: side, screens: screens)
        let peek = !tucked || PetPresence.wantsYou(shown ?? .idle)
        peeking = peek
        if !peek, let screen = PetPlacement.screen(for: target, among: screens) {
            target = PetPlacement.tucked(target, in: screen.visible)
        }
        guard target != window.frame else { return }
        if animated, !reduceMotion {
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.25
                window.animator().setFrame(target, display: false)
            }
        } else {
            window.setFrame(target, display: false)
        }
    }

    private func savePlacement() {
        if let data = try? JSONEncoder().encode(placement) { defaults.set(data, forKey: Self.placementKey) }
        defaults.set(tucked, forKey: Self.tuckedKey)
    }

    private func dropped() {
        guard let window else { return }
        let screens = Self.screens()
        guard let screen = PetPlacement.screen(for: window.frame, among: screens) else { return }
        let snapped = PetPlacement.snap(window.frame, in: screen.visible)
        placement.remember(snapped, on: screen)
        tucked = false
        savePlacement()
        place(animated: true)
        // The bubble rode along; once Mo settles, see which side it fits on now.
        after("bubbleLayout", 0.3) { $0.show() }
    }

    // MARK: - Sessions and events

    private func observeModel(_ generation: Int) {
        withObservationTracking {
            _ = model.sessions
        } onChange: { [weak self] in
            Task { @MainActor in
                guard let self, self.watching == generation, self.window != nil else { return }
                self.update()
                self.observeModel(generation)
            }
        }
    }

    /// New events from the app's one reader (`EventFeed`). Mo only looks for
    /// `restored` (the happy show); poses come from the snapshot, so a missed
    /// event never leaves Mo in the wrong pose.
    func received(_ events: [SessionEvent]) {
        guard window != nil else { return }
        if events.contains(where: { $0.kind == "restored" }) { celebrate() }
    }

    // MARK: - Do not disturb

    private var doNotDisturbUntil: Date? {
        let t = defaults.double(forKey: Self.doNotDisturbKey)
        guard t > 0 else { return nil }
        let until = Date(timeIntervalSince1970: t)
        return until > Date() ? until : nil
    }

    private func setDoNotDisturb(until: Date?) {
        if let until {
            defaults.set(until.timeIntervalSince1970, forKey: Self.doNotDisturbKey)
        } else {
            defaults.removeObject(forKey: Self.doNotDisturbKey)
        }
        update()
    }

    // MARK: - Pose

    /// The sessions or the settings changed: work out the pose again.
    private func update() {
        let dnd = doNotDisturbUntil
        stage.rest(PetPose.resting(model.sessions, doNotDisturb: dnd != nil))
        director.doNotDisturb = dnd != nil
        let sessions = model.sessions
        let clock = Date()
        director.keep { Self.stillTrue($0, in: sessions, now: clock) }
        if let dnd { after("dnd", dnd.timeIntervalSinceNow) { $0.update() } } else { cancel("dnd") }
        show()
    }

    private func celebrate() {
        if stage.celebrate(at: now) { show() }
    }

    private var visible: Bool { offScreen || (!occluded && !screensOff && !locked) }

    /// Bring the window and layers in line with the pose at this moment.
    private func show() {
        guard let view else { return }
        let t = now
        let pose = stage.pose(at: t)
        if pose != shown { enter(pose, previous: shown, at: t) }

        let loops = PetLoops.running(pose: pose, since: poseSince, now: t, reduceMotion: reduceMotion, visible: visible)
        view.setLoops(loops)
        if loops.contains(.blink) {
            if timers["blink"] == nil {
                after("blink", .random(in: 6...12)) { me in
                    me.view?.blink()
                    me.show()
                }
            }
        } else {
            cancel("blink")
        }
        if !loops.isEmpty, let still = PetLoops.stillAt(pose: pose, since: poseSince), still > t {
            after("still", still - t) { $0.show() }
        } else {
            cancel("still")
        }
        // Waiting for you: a light wave once a minute.
        if pose == .needsYou, visible {
            if timers["wave"] == nil {
                after("wave", 60) { me in
                    if me.reduceMotion { me.view?.flashLamp() } else { me.view?.wave(light: true) }
                    me.show()
                }
            }
        } else {
            cancel("wave")
        }
        if let end = stage.performanceEnds, end > t { after("show", end - t) { $0.show() } }

        let display = bubbleDisplay(at: t)
        let talking: Bool = if case .box = display { true } else { false }
        presence.update(pose: pose, hovering: hovering || onBubble || talking || menuOpen, at: t)
        let opacity = presence.opacity(at: t)
        if abs(view.presence - opacity) > 0.01 { view.setPresence(opacity, animated: !reduceMotion) }
        if let back = presence.fadeBackAt, back > t { after("fade", back - t) { $0.show() } }

        if tucked, peeking != PetPresence.wantsYou(pose) { place(animated: true) }
        view.setAccessibilityLabel(L.t("Mo: ", "小莫：") + pose.title)
        // No summary tooltip over a message Mo is holding for you.
        // No system tooltip (the yellow box clashes with everything else);
        // the summary still reaches VoiceOver.
        view.toolTip = nil
        view.setAccessibilityHelp(summary)
        render(display, at: t)
    }

    private func enter(_ pose: PetPose, previous: PetPose?, at t: TimeInterval) {
        guard let view else { return }
        shown = pose
        poseSince = t
        let first = previous == nil
        view.apply(PetLook.of(pose), animated: !first && !reduceMotion, fade: !first && reduceMotion)
        guard pose == .needsYou || pose == .happy else { return }
        if reduceMotion {
            view.flashLamp()
        } else {
            view.hop()
            if pose == .needsYou { view.wave() }
        }
    }

    /// "2 working · 1 needs you", for the tooltip.
    private var summary: String {
        let s = model.sessions
        var parts: [String] = []
        let needs = s.filter(\.needsYou).count
        let broken = s.filter { $0.state == .interrupted }.count
        let busy = s.filter { $0.state == .working }.count
        if needs > 0 { parts.append(L.t("\(needs) needs you", "\(needs) 需要你")) }
        if broken > 0 { parts.append(L.t("\(broken) interrupted", "\(broken) 断了")) }
        if busy > 0 { parts.append(L.t("\(busy) working", "\(busy) 在忙")) }
        return parts.isEmpty ? L.t("Mo: all quiet", "小莫：一切安好") : parts.joined(separator: " · ")
    }

    // MARK: - Bubbles

    /// What the director says, unless Mo can't be seen (then nothing, and
    /// the message waits).
    private func bubbleDisplay(at t: TimeInterval) -> PetDisplay {
        guard !offScreen, visible, !tucked else { return .nothing }
        return director.display(at: t)
    }

    private func render(_ display: PetDisplay, at t: TimeInterval) {
        guard let bubbles, let window, let anchor = bubbleAnchor() else { return }
        if menuOpen {
            let menu = menuContent()
            bubbles.menu(greeting: menu.greeting, groups: menu.groups, anchor: anchor.shoulder, parent: window, reduceMotion: reduceMotion)
            return
        }
        switch display {
        case .nothing:
            bubbles.hide(animated: !reduceMotion)
        case let .puff(id, text):
            bubbles.puff(text, id: id, anchor: anchor, parent: window, reduceMotion: reduceMotion)
        case let .box(message):
            bubbles.box(message, text: message.text(names: names(message)), anchor: anchor, parent: window,
                        reduceMotion: reduceMotion,
                        primary: { [weak self] in self?.perform(message) },
                        later: { [weak self] in self?.later() })
        }
        if let next = director.nextChange(after: t) { after("bubble", next - t) { $0.show() } } else { cancel("bubble") }
    }

    /// Mo's head and feet on screen, and the screen's usable area.
    private func bubbleAnchor() -> BubbleAnchor? {
        guard let window, let view, let screen = window.screen ?? NSScreen.main else { return nil }
        let shape = view.hitShape.boundingBox // y-down
        func screenPoint(_ p: CGPoint) -> CGPoint { window.convertPoint(toScreen: view.convert(p, to: nil)) }
        return BubbleAnchor(head: screenPoint(CGPoint(x: shape.midX, y: shape.minY - 8)),
                            feet: screenPoint(CGPoint(x: shape.midX, y: shape.maxY + 4)),
                            visible: screen.visibleFrame)
    }

    private func pointedAtBubble(_ inside: Bool) {
        onBubble = inside
        director.pointer(onBubble: inside, at: now)
        show()
    }

    /// The box's main button (or a click on its words): the same as the
    /// notification's (A23). One session: open it, or show it in the panel
    /// to bring it back. Several: the panel, on them.
    private func perform(_ message: PetMessage) {
        director.dismiss()
        show()
        if message.alerts.count == 1, let slot = message.slots.first {
            AlertCenter.perform(message.action, slot: slot, model: model, showPanel: showPanel)
            return
        }
        model.query = ""
        model.stateFilter = switch message.kind {
        case .interrupted: .interrupted
        case .finished: .waiting
        case .needsYou, .restored: nil
        }
        if let first = message.slots.first, model.sessions.contains(where: { $0.id == first }) { model.selectedID = first }
        showPanel()
    }

    /// "Later", "Not now", "Got it", or the close button.
    private func later() {
        director.dismiss()
        show()
    }

    /// The names the panel shows, for the message's sessions.
    private func names(_ message: PetMessage) -> [String] {
        message.alerts.map { alert in
            if let s = model.sessions.first(where: { $0.id == alert.slotId }) { return s.title }
            if let name = alert.name, !name.isEmpty { return name }
            return String(alert.slotId.prefix(8))
        }
    }

    /// Whether an alert still says something true. An alert can arrive a
    /// moment before the snapshot that shows it, so a fresh one is kept.
    static func stillTrue(_ alert: SessionAlert, in sessions: [Session], now: Date) -> Bool {
        if now.timeIntervalSince(alert.at) < 5 { return true }
        guard let session = sessions.first(where: { $0.id == alert.slotId }) else { return false }
        switch alert.kind {
        case .needsYou: return session.needsYou
        case .interrupted: return session.state == .interrupted
        case .restored, .finished: return true
        }
    }

    // MARK: - Mouse

    private func clicked(_ count: Int) {
        if menuOpen {
            closeMenu()
            return
        }
        switch count {
        case 0, 1: // 0: a click that wasn't counted (synthesized, or from accessibility)
            // Holding a message: the click opens it; otherwise the panel, as always.
            switch director.click(at: now) {
            case .showBox: show()
            case .openPanel: showPanel()
            }
            onClick?()
        case 3:
            celebrate() // the easter egg: a quiet little cheer
        default:
            break
        }
    }

    private func hovered(_ inside: Bool) {
        hovering = inside
        director.pointer(onMo: inside, at: now)
        if !inside { view?.look(toward: nil) }
        show()
        onHover?(inside)
    }

    /// Right-click: Mo asks what you'd like, in a bubble by its head (the
    /// pointer is already there), instead of a system menu. Again closes it.
    private func menu(_ event: NSEvent) {
        menuOpen ? closeMenu() : openMenu()
    }

    private func openMenu() {
        guard let window, visible else { return }
        menuOpen = true
        let mine = [window, bubbles?.window].compactMap { $0 }
        // A click anywhere else puts the bubble away: in other apps (global)
        // or in our own panel or settings (local). Clicks on Mo and the
        // bubble are theirs to handle.
        if let m = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown], handler: { [weak self] _ in
            MainActor.assumeIsolated { self?.closeMenu() }
        }) { menuMonitors.append(m) }
        if let m = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown], handler: { [weak self] event in
            MainActor.assumeIsolated {
                if let w = event.window, mine.contains(where: { $0 === w }) { return }
                self?.closeMenu()
            }
            return event
        }) { menuMonitors.append(m) }
        show()
    }

    private func closeMenu(render: Bool = true) {
        menuMonitors.forEach(NSEvent.removeMonitor)
        menuMonitors.removeAll()
        guard menuOpen else { return }
        menuOpen = false
        if render {
            bubbles?.hide(animated: false)
            show()
        }
    }

    private func menuContent() -> (greeting: String, groups: [[PetMenuChoice]]) {
        let s = model.sessions
        return PetMenu.content(needs: s.filter(\.needsYou).count, broken: s.filter { $0.state == .interrupted }.count,
                               resting: doNotDisturbUntil != nil, tucked: tucked) { [weak self] id in
            self?.choose(id)
        }
    }

    /// Close the bubble, then do what was picked.
    private func choose(_ id: PetMenu.ID) {
        closeMenu()
        switch id {
        case .panel: showPanel()
        case .wake: setDoNotDisturb(until: nil)
        case .hour: setDoNotDisturb(until: Date().addingTimeInterval(3600))
        case .today:
            let tomorrow = Calendar.current.date(byAdding: .day, value: 1, to: Date()) ?? Date()
            setDoNotDisturb(until: Calendar.current.startOfDay(for: tomorrow))
        case .tuck:
            tucked.toggle()
            savePlacement()
            place(animated: true)
        case .settings:
            SettingsView.show(.mo)
            openSettings?(id: "settings")
            SettingsWindow.bringToFront()
        case .hide:
            defaults.set(false, forKey: Self.enabledKey)
            NotificationCenter.default.post(name: Self.settingsChanged, object: nil)
        }
    }

    // MARK: - System

    private func watchSystem(_ panel: NSWindow) {
        let ws = NSWorkspace.shared.notificationCenter
        let dist = DistributedNotificationCenter.default()
        let local = NotificationCenter.default
        func on(_ center: NotificationCenter, _ name: Notification.Name, _ object: AnyObject? = nil,
                _ body: @escaping @MainActor (PetController) -> Void) {
            let o = center.addObserver(forName: name, object: object, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { if let self { body(self) } }
            }
            observers.append((center, o))
        }
        on(ws, NSWorkspace.screensDidSleepNotification) { $0.screensOff = true; $0.show() }
        on(ws, NSWorkspace.screensDidWakeNotification) { $0.screensOff = false; $0.show() }
        on(dist, Notification.Name("com.apple.screenIsLocked")) { $0.locked = true; $0.show() }
        on(dist, Notification.Name("com.apple.screenIsUnlocked")) { $0.locked = false; $0.show() }
        on(ws, NSWorkspace.accessibilityDisplayOptionsDidChangeNotification) { me in
            me.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
            me.show()
        }
        on(local, NSApplication.didChangeScreenParametersNotification) { $0.place() }
        on(local, NSWindow.didChangeOcclusionStateNotification, panel) { me in
            me.occluded = !(me.window?.occlusionState.contains(.visible) ?? false)
            me.show()
        }
    }

    // MARK: - Timers

    /// One-shot timer under a name; setting it again replaces it.
    private func after(_ key: String, _ delay: TimeInterval, _ body: @escaping @MainActor (PetController) -> Void) {
        timers[key]?.invalidate()
        let timer = Timer(timeInterval: max(delay, 0.01), repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.timers[key] = nil
                body(self)
            }
        }
        timer.tolerance = max(0.05, delay * 0.1)
        RunLoop.main.add(timer, forMode: .common)
        timers[key] = timer
    }

    private func cancel(_ key: String) {
        timers.removeValue(forKey: key)?.invalidate()
    }

    /// For the bench: which loops are on and which timers are waiting.
    var activity: String {
        "loops=\(view?.runningLoops.joined(separator: ",") ?? "-") timers=\(timers.keys.sorted().joined(separator: ","))"
    }
}

extension NSScreen {
    /// The display's UUID: the same screen keeps it across launches and ports.
    var petID: String {
        guard let number = deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber else { return localizedName }
        guard let uuid = CGDisplayCreateUUIDFromDisplayID(number.uint32Value)?.takeRetainedValue() else { return "\(number)" }
        return (CFUUIDCreateString(nil, uuid) as String?) ?? "\(number)"
    }
}
