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

    init(model: AppModel, showPanel: @escaping () -> Void, defaults: UserDefaults = .standard, offScreen: Bool = false) {
        self.model = model
        self.showPanel = showPanel
        self.defaults = defaults
        self.offScreen = offScreen
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

    /// The router decided this one is Mo's to tell.
    func take(_ alert: SessionAlert) {
        alerts.append(alert)
        if alerts.count > 20 { alerts.removeFirst(alerts.count - 20) }
        onAlert?(alert)
    }

    static func enabled(_ defaults: UserDefaults = .standard) -> Bool { defaults.bool(forKey: enabledKey) }

    static func size(_ defaults: UserDefaults = .standard) -> CGFloat {
        let v = CGFloat(defaults.double(forKey: sizeKey))
        return sizes.contains(v) ? v : defaultSize
    }

    private var now: TimeInterval { ProcessInfo.processInfo.systemUptime }

    /// Show, hide or resize Mo to match the settings.
    func sync() {
        let want = Self.enabled(defaults)
        let wantSize = Self.size(defaults)
        if window != nil, !want || wantSize != size { teardown() }
        if want, window == nil { build(size: wantSize) }
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

        presence.update(pose: pose, hovering: hovering, at: t)
        let opacity = presence.opacity(at: t)
        if abs(view.presence - opacity) > 0.01 { view.setPresence(opacity, animated: !reduceMotion) }
        if let back = presence.fadeBackAt, back > t { after("fade", back - t) { $0.show() } }

        if tucked, peeking != PetPresence.wantsYou(pose) { place(animated: true) }
        view.setAccessibilityLabel(L.t("Mo: ", "小莫：") + pose.title)
        view.toolTip = summary
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

    // MARK: - Mouse

    private func clicked(_ count: Int) {
        switch count {
        case 1:
            showPanel()
            onClick?()
        case 3:
            celebrate() // the easter egg: a quiet little cheer
        default:
            break
        }
    }

    private func hovered(_ inside: Bool) {
        hovering = inside
        if !inside { view?.look(toward: nil) }
        show()
        onHover?(inside)
    }

    private func menu(_ event: NSEvent) {
        guard let view else { return }
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.addItem(PetMenuItem(L.t("Open Panel", "打开面板"), symbol: "rectangle.on.rectangle") { [weak self] in self?.showPanel() })
        menu.addItem(.separator())
        if doNotDisturbUntil != nil {
            menu.addItem(PetMenuItem(L.t("Wake Mo Up", "叫醒小莫"), symbol: "sun.max") { [weak self] in
                self?.setDoNotDisturb(until: nil)
            })
        } else {
            menu.addItem(PetMenuItem(L.t("Do Not Disturb for an Hour", "勿扰一小时"), symbol: "moon") { [weak self] in
                self?.setDoNotDisturb(until: Date().addingTimeInterval(3600))
            })
            menu.addItem(PetMenuItem(L.t("Do Not Disturb Today", "今天勿扰"), symbol: "moon.zzz") { [weak self] in
                let tomorrow = Calendar.current.date(byAdding: .day, value: 1, to: Date()) ?? Date()
                self?.setDoNotDisturb(until: Calendar.current.startOfDay(for: tomorrow))
            })
        }
        menu.addItem(PetMenuItem(tucked ? L.t("Come Out", "探出来") : L.t("Tuck Away", "收到边上"),
                                 symbol: tucked ? "arrow.left.to.line" : "arrow.right.to.line") { [weak self] in
            guard let self else { return }
            self.tucked.toggle()
            self.savePlacement()
            self.place(animated: true)
        })
        menu.addItem(.separator())
        menu.addItem(PetMenuItem(L.t("Pet Settings…", "宠物设置…"), symbol: "gearshape") { [weak self] in
            self?.openSettings?(id: "settings")
            SettingsWindow.bringToFront()
        })
        menu.addItem(PetMenuItem(L.t("Hide Mo", "隐藏小莫"), symbol: "eye.slash") { [weak self] in
            guard let self else { return }
            self.defaults.set(false, forKey: Self.enabledKey)
            NotificationCenter.default.post(name: Self.settingsChanged, object: nil)
        })
        NSMenu.popUpContextMenu(menu, with: event, for: view)
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
