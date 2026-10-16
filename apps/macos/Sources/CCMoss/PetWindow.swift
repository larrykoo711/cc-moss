import AppKit
import MossKit
import SwiftUI

/// The window Mo lives in (DESIGN-pet §8): transparent, borderless, above
/// ordinary windows, never taking the keyboard or activating the app, on
/// every Space but not over full-screen apps (no `.fullScreenAuxiliary`).
final class PetPanel: NSPanel {
    init(side: CGFloat) {
        super.init(contentRect: NSRect(x: 0, y: 0, width: side, height: side),
                   styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        isFloatingPanel = true
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
        backgroundColor = .clear
        isOpaque = false
        // Mo draws its own shadows, shaped like it; a window shadow would be a square.
        hasShadow = false
        becomesKeyOnlyIfNeeded = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        isMovable = false
        animationBehavior = .none
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

/// How clicks on Mo's transparent surroundings reach the window below.
enum PetClickThrough: String {
    /// The window server skips fully transparent pixels of a non-opaque
    /// window; `PetView.hitTest` turns down the rest. No work at all while
    /// the pointer is elsewhere. The default.
    case alpha
    /// Fallback if `alpha` turns out unreliable on some system: the window
    /// ignores the mouse unless the pointer is over Mo, which needs a global
    /// mouse-moved monitor (a little work on every pointer move).
    case toggle

    static let key = "petClickThrough"

    static var current: PetClickThrough {
        UserDefaults.standard.string(forKey: key).flatMap(PetClickThrough.init) ?? .alpha
    }
}

/// The `.toggle` fallback: ignore the mouse except over Mo's shape.
@MainActor
final class PetMouseGate {
    private var monitors: [Any] = []

    init(window: NSWindow, view: PetView) {
        window.ignoresMouseEvents = true
        let update: (NSEvent) -> Void = { [weak window, weak view] _ in
            MainActor.assumeIsolated {
                guard let window, let view else { return }
                let p = NSEvent.mouseLocation
                guard window.frame.contains(p) else {
                    if !window.ignoresMouseEvents { window.ignoresMouseEvents = true }
                    return
                }
                let local = view.convert(window.convertPoint(fromScreen: p), from: nil)
                window.ignoresMouseEvents = !view.hitShape.contains(local)
            }
        }
        let kinds: NSEvent.EventTypeMask = [.mouseMoved, .leftMouseDragged]
        if let m = NSEvent.addGlobalMonitorForEvents(matching: kinds, handler: update) { monitors.append(m) }
        if let m = NSEvent.addLocalMonitorForEvents(matching: kinds, handler: { update($0); return $0 }) { monitors.append(m) }
    }

    func stop() {
        monitors.forEach(NSEvent.removeMonitor)
        monitors.removeAll()
    }
}

/// Lets AppKit code open the SwiftUI settings window: a hidden SwiftUI view
/// in Mo's window hands over its `openWindow`.
struct SettingsOpener: View {
    let receive: @MainActor (OpenWindowAction) -> Void
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Color.clear.frame(width: 0, height: 0).onAppear { receive(openWindow) }
    }
}

/// A menu item that runs a closure.
final class PetMenuItem: NSMenuItem {
    private let run: () -> Void

    init(_ title: String, symbol: String? = nil, run: @escaping () -> Void) {
        self.run = run
        super.init(title: title, action: #selector(fire), keyEquivalent: "")
        target = self
        if let symbol { image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil) }
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError("not used") }

    @objc private func fire() { run() }
}
