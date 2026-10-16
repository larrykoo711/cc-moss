import AppKit
import MossKit
import SwiftUI

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
        motion.isHidden = true
        button.addSubview(motion)
        refresh()
        icon.onChange = { [weak self] in self?.refresh() }
    }

    /// The animated mark, layered over the button's image.
    private let motion = IconMotionView(frame: NSRect(x: 0, y: 0, width: StatusIcon.side, height: StatusIcon.side))

    private func refresh() {
        guard let button = item.button else { return }
        if button.image !== icon.image { button.image = icon.image }
        if motion.playing != icon.moving {
            if let state = icon.moving { motion.play(state) } else { motion.stop() }
        }
        if icon.moving != nil { placeMotion(in: button) }
        let label = "cc-moss: " + icon.state.label
        if button.accessibilityLabel() != label { button.setAccessibilityLabel(label) }
    }

    /// Over the mark's square: the left end of the image, centred vertically.
    private func placeMotion(in button: NSStatusBarButton) {
        button.layoutSubtreeIfNeeded()
        let bounds = button.bounds
        let size = icon.image.size
        let imageRect = button.cell?.imageRect(forBounds: bounds)
            ?? NSRect(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2, width: size.width, height: size.height)
        // The cell may centre a smaller image inside a larger rect.
        let x = imageRect.midX - size.width / 2
        let y = imageRect.midY - StatusIcon.side / 2
        let frame = NSRect(x: x.rounded(), y: y.rounded(), width: StatusIcon.side, height: StatusIcon.side)
        if motion.frame != frame { motion.frame = frame }
    }

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
        orderOut(nil)
        onClose?()
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

    static let cornerRadius: CGFloat = 14

    init(model: AppModel) {
        self.model = model
    }

    func toggle() {
        if isShown {
            hide()
        } else if ProcessInfo.processInfo.systemUptime - hiddenAt > 0.25 {
            show()
        }
    }

    func show() {
        let panel = window ?? makeWindow()
        window = panel
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

    func hide() {
        guard isShown else { return }
        window?.orderOut(nil)
        didHide()
    }

    private func didHide() {
        guard isShown else { return }
        isShown = false
        hiddenAt = ProcessInfo.processInfo.systemUptime
        stopWatching()
        onVisibilityChange?(false)
    }

    private func makeWindow() -> KeyPanel {
        let size = NSSize(width: Theme.panelWidth, height: Theme.panelHeight)
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
            .environment(\.closePanel) { [weak self] in self?.hide() })
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
    private func position(_ panel: NSPanel) {
        let size = panel.frame.size
        let button = anchor?()
        let screen = NSScreen.screens.first { s in button.map { s.frame.intersects($0) } ?? false } ?? NSScreen.main
        guard let visible = screen?.visibleFrame else { return }
        let top = (button?.minY ?? visible.maxY) - 5
        var x = button?.minX ?? (visible.maxX - size.width - 8)
        x = min(max(x, visible.minX + 8), visible.maxX - size.width - 8)
        panel.setFrameTopLeftPoint(NSPoint(x: x, y: top))
    }

    private func startWatching(_ panel: NSPanel) {
        // A click anywhere in another app.
        if let m = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown], handler: { [weak self] _ in
            MainActor.assumeIsolated { self?.hide() }
        }) {
            monitors.append(m)
        }
        let center = NotificationCenter.default
        // Another window of ours (Settings) or another app took the keyboard.
        observers.append(center.addObserver(forName: NSWindow.didResignKeyNotification, object: panel, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.hide() }
        })
        observers.append(center.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.hide() }
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

    /// The panel, for code that wants to open it (hotkey, notifications).
    var panel: PanelController? { statusItem?.panel }

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = StatusItemController(model: model)
    }
}
