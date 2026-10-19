import AppKit
import MossKit
import OSLog
import SwiftUI

/// cc-moss:// links on their way to the panel, for watching a cold start:
/// `log stream --level debug --predicate 'subsystem == "io.github.larrykoo711.cc-moss" && category == "link"'`
let linkLog = Logger(subsystem: "io.github.larrykoo711.cc-moss", category: "link")

/// Owns the menu bar item. We drive an `NSStatusItem` ourselves rather than
/// SwiftUI's `MenuBarExtra`: a label change there costs a full scene update
/// per frame, while setting `button.image` is all an animation frame needs.
@MainActor
final class StatusItemController {
    let panel: PanelController
    private let item: NSStatusItem
    private let icon: MenuBarIcon

    init(model: AppModel) {
        icon = model.icon
        item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        panel = PanelController(model: model)
        guard let button = item.button else { return }
        button.imagePosition = .imageOnly
        button.target = self
        button.action = #selector(clicked)
        // Open on press, like a menu.
        button.sendAction(on: [.leftMouseDown, .rightMouseDown])
        panel.anchor = { [weak self] in self?.buttonFrame }
        panel.onVisibilityChange = { [weak button] shown in button?.highlight(shown) }
        show(icon.image)
        icon.onImage = { [weak self] image in self?.show(image) }
    }

    private func show(_ image: NSImage) {
        guard let button = item.button else { return }
        button.image = image
        let label = "cc-moss: " + icon.state.label
        if button.accessibilityLabel() != label { button.setAccessibilityLabel(label) }
    }

    /// The status button is in the menu bar (it has a window): the panel can
    /// be placed under it.
    var isPlaced: Bool { item.button?.window != nil }

    /// The button's rectangle on screen.
    private var buttonFrame: NSRect? {
        guard let button = item.button, let window = button.window else { return nil }
        return window.convertToScreen(button.convert(button.bounds, to: nil))
    }

    @objc private func clicked() {
        panel.toggle()
    }
}

/// A borderless panel that can take the keyboard (for the search field)
/// without a title bar; `close()` (Esc in the panel, opening Settings) only hides it.
final class KeyPanel: NSPanel {
    var onClose: (() -> Void)?

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    override func close() {
        linkLog.debug("panel close() called")
        orderOut(nil)
        onClose?()
    }

    override func orderOut(_ sender: Any?) {
        if isVisible {
            let caller = Thread.callStackSymbols.dropFirst().prefix(6).joined(separator: " | ")
            linkLog.debug("panel ordered out; from: \(caller, privacy: .public)")
        }
        super.orderOut(sender)
    }

    override func resignKey() {
        super.resignKey()
        let next = NSApp.keyWindow.map { "\(type(of: $0)) \($0.title)" } ?? "none"
        linkLog.debug("panel resigned key; key window now: \(next, privacy: .public), app active: \(NSApp.isActive)")
    }

    override func cancelOperation(_ sender: Any?) {
        close()
    }
}

/// The sessions panel under the menu bar item. `show()` / `toggle()` are
/// public so a global hotkey or a notification click can open it too.
@MainActor
final class PanelController {
    /// Where the status button is on screen; nil puts the panel under the menu bar's right side.
    var anchor: (() -> NSRect?)?
    var onVisibilityChange: ((Bool) -> Void)?
    private(set) var isShown = false

    private let model: AppModel
    private var window: KeyPanel?
    private var monitors: [Any] = []
    private var observers: [NSObjectProtocol] = []
    /// A click on the status button first takes the key away from the
    /// panel (hiding it), then toggles: don't reopen it straight away.
    private var hiddenAt: TimeInterval = 0
    /// Until then, losing the key or the app's activation doesn't hide the
    /// panel: a link that launched the app opens it while the launch is
    /// still settling who's frontmost.
    private var steadyUntil: TimeInterval = 0

    /// Shown and visible on screen.
    var isVisible: Bool { isShown && window?.isVisible == true }

    /// Show it, and keep it through the next `seconds` of focus changes.
    func show(holding seconds: TimeInterval) {
        steadyUntil = ProcessInfo.processInfo.systemUptime + seconds
        show()
    }

    private var steady: Bool { ProcessInfo.processInfo.systemUptime < steadyUntil }

    static let cornerRadius: CGFloat = 14

    init(model: AppModel) {
        self.model = model
    }

    func toggle() {
        if isShown {
            hide("toggled")
        } else if ProcessInfo.processInfo.systemUptime - hiddenAt > 0.25 {
            show()
        }
    }

    func show() {
        let panel = window ?? makeWindow()
        window = panel
        // Its size is picked as it opens, never while it's open.
        if !isShown, let visible = screen()?.visibleFrame {
            let height = model.sizePanel(visible: visible.height)
            if abs(panel.frame.height - height) > 0.5 {
                panel.setContentSize(NSSize(width: Theme.panelWidth, height: height))
            }
        }
        position(panel)
        // Key without activating the app: the search field takes typing at
        // once and the app you were in stays frontmost.
        panel.makeKeyAndOrderFront(nil)
        panel.invalidateShadow()
        guard !isShown else { return }
        isShown = true
        startWatching(panel)
        onVisibilityChange?(true)
    }

    func hide(_ reason: String = "asked") {
        guard isShown else { return }
        linkLog.debug("panel hidden: \(reason, privacy: .public)")
        window?.orderOut(nil)
        didHide()
    }

    /// Show it again when it is meant to be up but isn't on screen (ordered
    /// out by something that didn't go through `hide`).
    func showAgain(holding seconds: TimeInterval) {
        if isShown, window?.isVisible != true {
            linkLog.debug("panel was shown but not on screen: resetting")
            isShown = false
            stopWatching()
        }
        show(holding: seconds)
        linkLog.debug("panel shown again; visible: \(self.isVisible)")
    }

    private func didHide() {
        guard isShown else { return }
        isShown = false
        hiddenAt = ProcessInfo.processInfo.systemUptime
        stopWatching()
        onVisibilityChange?(false)
    }

    private func makeWindow() -> KeyPanel {
        let size = NSSize(width: Theme.panelWidth, height: model.panelHeight)
        let panel = KeyPanel(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered, defer: true
        )
        panel.isFloatingPanel = true
        panel.level = .statusBar
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        panel.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary, .transient]
        panel.animationBehavior = .utilityWindow
        panel.onClose = { [weak self] in self?.didHide() }

        // Frosted glass behind the panel's own near-opaque surface, rounded.
        let glass = NSVisualEffectView(frame: NSRect(origin: .zero, size: size))
        glass.material = .popover
        glass.blendingMode = .behindWindow
        glass.state = .active
        glass.maskImage = Self.roundedMask(radius: Self.cornerRadius)
        glass.autoresizingMask = [.width, .height]
        let host = NSHostingView(rootView: PanelView(model: model)
            .environment(\.closePanel) { [weak self] in self?.hide("closed from inside (Esc, opened something)") }
            .followsLanguage())
        host.frame = glass.bounds
        host.autoresizingMask = [.width, .height]
        host.wantsLayer = true
        host.layer?.cornerRadius = Self.cornerRadius
        host.layer?.cornerCurve = .continuous
        host.layer?.masksToBounds = true
        glass.addSubview(host)
        panel.contentView = glass
        return panel
    }

    /// Under the status button, left edges aligned, kept on its screen.
    /// The screen the status button is on.
    private func screen() -> NSScreen? {
        let button = anchor?()
        return NSScreen.screens.first { s in button.map { s.frame.intersects($0) } ?? false } ?? NSScreen.main
    }

    private func position(_ panel: NSPanel) {
        let size = panel.frame.size
        let button = anchor?()
        guard let visible = screen()?.visibleFrame else { return }
        let top = (button?.minY ?? visible.maxY) - 5
        var x = button?.minX ?? (visible.maxX - size.width - 8)
        x = min(max(x, visible.minX + 8), visible.maxX - size.width - 8)
        panel.setFrameTopLeftPoint(NSPoint(x: x, y: top))
    }

    private func startWatching(_ panel: NSPanel) {
        // A click anywhere in another app.
        if let m = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown], handler: { [weak self] _ in
            MainActor.assumeIsolated {
                // A dropdown open over the panel closes on the same click;
                // look again once it has.
                guard PanelHold.active else { self?.hide("a click in another app"); return }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
                    if !PanelHold.active { self?.hide("a click in another app") }
                }
            }
        }) {
            monitors.append(m)
        }
        let center = NotificationCenter.default
        // Another window of ours (Settings) or another app took the keyboard;
        // not our own dropdown or folder picker over the panel (PanelHold).
        observers.append(center.addObserver(forName: NSWindow.didResignKeyNotification, object: panel, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !PanelHold.active else { return }
                if self.steady { linkLog.debug("resign key ignored (just opened by a link)"); return }
                self.hide("resigned key")
            }
        })
        observers.append(center.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !PanelHold.active else { return }
                if self.steady { linkLog.debug("resign active ignored (just opened by a link)"); return }
                self.hide("app resigned active")
            }
        })
    }

    private func stopWatching() {
        monitors.forEach(NSEvent.removeMonitor)
        monitors.removeAll()
        observers.forEach(NotificationCenter.default.removeObserver)
        observers.removeAll()
    }

    /// A stretchable rounded-rectangle mask for the glass.
    private static func roundedMask(radius: CGFloat) -> NSImage {
        let edge = radius * 2 + 1
        let image = NSImage(size: NSSize(width: edge, height: edge), flipped: false) { rect in
            NSColor.black.setFill()
            NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
            return true
        }
        image.capInsets = NSEdgeInsets(top: radius, left: radius, bottom: radius, right: radius)
        image.resizingMode = .stretch
        return image
    }
}

/// How the panel's content hides its window (Esc, opening Settings). The
/// default closes whatever window has the keyboard, for previews.
private struct ClosePanelKey: EnvironmentKey {
    static let defaultValue: @MainActor () -> Void = { NSApplication.shared.keyWindow?.close() }
}

extension EnvironmentValues {
    var closePanel: @MainActor () -> Void {
        get { self[ClosePanelKey.self] }
        set { self[ClosePanelKey.self] = newValue }
    }
}

/// Creates the status item once the app has launched.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = AppModel()
    private(set) var statusItem: StatusItemController?
    /// Mo, the desktop pet; present (and invisible) even while switched off.
    private(set) var pet: PetController?

    /// The panel, for code that wants to open it (hotkey, notifications).
    var panel: PanelController? { statusItem?.panel }

    /// The one reader of the event stream, and where its alerts go (T2.3).
    private var events: EventFeed?
    /// A cc-moss:// link that arrived before the panel existed (a cold start).
    private var pendingLink: PanelLink?
    private var alerts: AlertCenter?
    private var startup: StartupReminder?

    /// Before SwiftUI sees any event: links come to us, and the chosen
    /// appearance is on before the first window draws.
    func applicationWillFinishLaunching(_ notification: Notification) {
        AppAppearance.apply()
        takeLinks()
    }

    /// `cc-moss://` links as an Apple Event, handled here. Left to SwiftUI,
    /// a link went to the Settings window scene and opened Settings too.
    private func takeLinks() {
        NSAppleEventManager.shared().setEventHandler(self, andSelector: #selector(openLink(_:reply:)),
                                                     forEventClass: AEEventClass(kInternetEventClass),
                                                     andEventID: AEEventID(kAEGetURL))
        linkLog.debug("link handler installed")
    }

    @objc private func openLink(_ event: NSAppleEventDescriptor, reply: NSAppleEventDescriptor) {
        guard let raw = event.paramDescriptor(forKeyword: keyDirectObject)?.stringValue, let url = URL(string: raw) else { return }
        linkLog.debug("link event received: \(raw, privacy: .public)")
        application(NSApp, open: [url])
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // SwiftUI may set its own link handler while launching: ours again.
        takeLinks()
        // Dev only: `defaults write io.github.larrykoo711.cc-moss DevShowInDock -bool true` shows a Dock icon so screen-control tools find the app.
        if UserDefaults.standard.bool(forKey: "DevShowInDock") { NSApp.setActivationPolicy(.regular) }
        statusItem = StatusItemController(model: model)
        watchFocusForLinks()
        if let link = pendingLink {
            pendingLink = nil
            linkLog.debug("pending link handled")
            DispatchQueue.main.async { [weak self] in self?.open(link) }
        }
        pet = PetController(model: model) { [weak self] in self?.panel?.show() }
        startAlerts()
        // The global shortcut does what a click on the icon does.
        HotKeyCenter.shared.onPress = { [weak self] in
            // During the welcome, the shortcut proves itself there.
            if WelcomeWindow.isShown, let tried = WelcomeWindow.onShortcut { tried() } else { self?.panel?.toggle() }
        }
        HotKeyCenter.shared.start()
        // An update can take Accessibility away; the panel says so.
        PermissionCenter.shared.checkAtLaunch(terminals: model.terminals)
        startLanguage()
        // The first-run welcome, once (T4.2).
        WelcomeWindow.openPanel = { [weak self] in self?.panel?.show() }
        // A moment after launch, once the menu bar icon has settled.
        if WelcomeFlow.shouldShow(.standard) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [weak self] in
                guard let self else { return }
                WelcomeWindow.show(model: self.model)
            }
        }
    }

    /// `[ui] language` from the CLI's config; a switch reaches the CLI's own
    /// answers too (its client runs with the app's language).
    private func startLanguage() {
        LanguageState.shared.onChange = { [weak self] in self?.model.relocate() }
        guard let client = model.client else { return }
        Task { await LanguageState.shared.load(from: client) }
    }

    /// Opened again (Spotlight, `open -a cc-moss`, the Dock icon when it
    /// shows): the panel, as a click on the menu bar icon would. Handy when
    /// the icon is hidden behind the notch or other icons.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        panel?.show()
        return false
    }

    /// `cc-moss://panel[/sessions|queue|moss]`: the panel, on that page.
    func application(_ application: NSApplication, open urls: [URL]) {
        guard let link = urls.lazy.compactMap(PanelLink.init).first else { return }
        // Launched by the link: it comes before the panel exists. Kept until
        // the menu bar icon (and its panel) is up.
        guard panel != nil else {
            pendingLink = link
            linkLog.debug("link kept until the panel exists")
            return
        }
        open(link)
    }

    private func open(_ link: PanelLink) {
        switch link {
        case .panel: break
        case .sessions: model.page = .sessions
        case .queue: model.showQueue()
        case .moss: model.showMoss()
        }
        showForLink(tries: 10)
    }

    /// Debug (category "link"): who takes the keyboard and the app's
    /// activation around a launch, to find what hides a link's panel.
    private func watchFocusForLinks() {
        let center = NotificationCenter.default
        let describe: (Any?) -> String = { object in
            guard let w = object as? NSWindow else { return "?" }
            return "\(type(of: w)) \"\(w.title)\" visible: \(w.isVisible)"
        }
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.willCloseNotification] {
            _ = center.addObserver(forName: name, object: nil, queue: .main) { note in
                linkLog.debug("\(note.name.rawValue, privacy: .public): \(describe(note.object), privacy: .public)")
            }
        }
        for name in [NSApplication.didBecomeActiveNotification, NSApplication.didResignActiveNotification,
                     NSApplication.didHideNotification, NSApplication.didUnhideNotification] {
            _ = center.addObserver(forName: name, object: nil, queue: .main) { note in
                linkLog.debug("\(note.name.rawValue, privacy: .public)")
            }
        }
    }

    /// Show the panel for a link: once the status button is in the menu bar
    /// (right after a cold launch it may not be yet; tried every 0.1 s for
    /// about a second), held through the launch's focus changes, and shown
    /// again once if it didn't stay up.
    private func showForLink(tries: Int) {
        guard let statusItem else { return }
        guard statusItem.isPlaced || tries <= 0 else {
            linkLog.debug("status button not placed yet; trying again")
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in self?.showForLink(tries: tries - 1) }
            return
        }
        statusItem.panel.show(holding: 1.5)
        linkLog.debug("panel shown for a link; visible: \(statusItem.panel.isVisible)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in
            guard let panel = self?.statusItem?.panel else { return }
            linkLog.debug("panel still visible after a link: \(panel.isVisible)")
            if !panel.isVisible { panel.showAgain(holding: 1.5) }
        }
    }

    /// A menu bar app lives on with no window open. SwiftUI otherwise answers
    /// yes here (Settings is its only window scene), so Mo hiding for a moment
    /// after a click, with Settings closed, quit the whole app.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    private func startAlerts() {
        let notifier = SystemNotifier.available ? SystemNotifier() : nil
        notifier?.onAction = { [weak self] action, slot in
            guard let self else { return }
            AlertCenter.perform(action, slot: slot, model: self.model) { self.panel?.show() }
        }
        notifier?.onAsk = { [weak self] id in
            guard let self else { return }
            self.model.focusAsk(id)
            self.panel?.show()
        }
        let alerts = AlertCenter(model: model, notifier: notifier, context: { [weak self] in
            AlertContext(
                mode: AlertMode(defaults: .standard),
                petEnabled: PetController.enabled(),
                petVisible: self?.pet?.isVisibleToYou ?? false,
                panelOpen: self?.panel?.isShown ?? false,
                doNotDisturb: PetController.doNotDisturb()
            )
        }, toPet: { [weak self] alert in self?.pet?.take(alert) })
        let feed = EventFeed(model: model)
        feed.listen { [weak self] events in
            self?.pet?.received(events)
            alerts.handle(events)
        }
        feed.start()
        model.onAsks = { alerts.handleAsks($0) }
        model.onMossReplies = { alerts.handleMossReplies($0) }
        let highlight = panel?.onVisibilityChange
        panel?.onVisibilityChange = { [weak self] shown in
            highlight?(shown)
            self?.model.panelShown = shown
            if !shown { alerts.panelClosed() }
        }
        self.alerts = alerts
        self.events = feed
        let startup = StartupReminder(model: model) { alerts.remindMissed($0) }
        startup.start()
        self.startup = startup
    }
}
