import AppKit
import MossKit
import SwiftUI

extension View {
    /// Our tooltip instead of the system's yellow box: a small rounded card
    /// in the panel's colours, after a short pause, never clipped by the
    /// view it belongs to (it floats in its own window). VoiceOver reads the
    /// same words as a hint.
    func tip(_ text: String) -> some View {
        modifier(Tip(text: text))
    }
}

private struct Tip: ViewModifier {
    let text: String
    @State private var anchor = AnchorBox()

    func body(content: Content) -> some View {
        content
            .background(AnchorView(box: anchor))
            .onHover { inside in
                if inside {
                    TooltipWindow.schedule(text, anchor: anchor)
                } else {
                    TooltipWindow.cancel(anchor)
                }
            }
            .onDisappear { TooltipWindow.cancel(anchor) }
            .accessibilityHint(text)
    }
}

/// The one tooltip on screen. Shows after 0.6 s of hovering, hides at once
/// on leaving, on a click, or when the pointer settles somewhere else.
@MainActor
enum TooltipWindow {
    private static var panel: NSPanel?
    private static var pending: DispatchWorkItem?
    private static weak var owner: AnchorBox?
    private static var clickMonitor: Any?

    static let delay: TimeInterval = 0.6

    static func schedule(_ text: String, anchor: AnchorBox) {
        cancel(nil)
        owner = anchor
        let work = DispatchWorkItem { [weak anchor] in
            MainActor.assumeIsolated {
                guard let anchor, owner === anchor else { return }
                show(text, anchor: anchor)
            }
        }
        pending = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    /// Hide the tooltip; with an anchor, only if it is that anchor's.
    static func cancel(_ anchor: AnchorBox?) {
        if let anchor, owner !== anchor { return }
        pending?.cancel()
        pending = nil
        owner = nil
        panel?.orderOut(nil)
        if let m = clickMonitor {
            NSEvent.removeMonitor(m)
            clickMonitor = nil
        }
    }

    private static func show(_ text: String, anchor: AnchorBox) {
        guard let view = anchor.view, let window = view.window, window.isVisible else { return }
        let rect = window.convertToScreen(view.convert(view.bounds, to: nil))
        let host = NSHostingView(rootView: TooltipCard(text: text)
            .environment(\.colorScheme, window.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? .dark : .light))
        let size = host.fittingSize
        let p = panel ?? makePanel()
        p.contentView = host
        p.setContentSize(size)
        // Centred under the control, or above it near the bottom of the screen.
        let screen = window.screen ?? NSScreen.main
        let visible = screen?.visibleFrame ?? rect
        var origin = NSPoint(x: rect.midX - size.width / 2, y: rect.minY - size.height - 6)
        if origin.y < visible.minY { origin.y = rect.maxY + 6 }
        origin.x = min(max(origin.x, visible.minX + 4), visible.maxX - size.width - 4)
        p.setFrameOrigin(origin)
        p.orderFront(nil)
        panel = p
        clickMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .keyDown]) { e in
            cancel(nil)
            return e
        }
    }

    private static func makePanel() -> NSPanel {
        let p = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = false
        p.level = .popUpMenu
        p.ignoresMouseEvents = true
        p.collectionBehavior = [.canJoinAllSpaces, .transient, .ignoresCycle]
        return p
    }
}

/// The card itself: small type, the panel's raised surface, a hairline and
/// a soft shadow that fits inside its window.
private struct TooltipCard: View {
    let text: String
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(.primary)
            .multilineTextAlignment(.leading)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: 260, alignment: .leading)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Theme.raised(scheme)))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Theme.hairline(scheme)))
            .shadow(color: .black.opacity(scheme == .dark ? 0.35 : 0.12), radius: 6, y: 2)
            .padding(14)
            .fixedSize()
    }
}
