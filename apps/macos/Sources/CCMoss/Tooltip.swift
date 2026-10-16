import AppKit
import MossKit
import SwiftUI

extension View {
    /// Our tooltip instead of the system's yellow box: a small rounded card
    /// in the panel's colours, drawn in the window's own top layer (see
    /// `tooltipLayer()`), so no scroll area clips it and no extra window
    /// shows its frame. VoiceOver reads the same words as a hint.
    func tip(_ text: String) -> some View {
        modifier(Tip(text: text))
    }

    /// Where a window's tooltips are drawn; put it on the window's root view.
    func tooltipLayer() -> some View {
        modifier(TooltipLayer())
    }
}

/// The one tooltip of a window. Timing follows macOS: it appears after the
/// pointer has rested for the system's initial tool tip delay (1 s unless
/// the user set `NSInitialToolTipDelay`), so sweeping across controls shows
/// nothing; once one is up, the next one within 0.5 s shows at once.
@MainActor @Observable
final class TooltipCenter {
    private(set) var text: String?
    private(set) var anchor: CGRect = .zero

    @ObservationIgnored private var owner: UUID?
    @ObservationIgnored private var latest: CGRect = .zero
    @ObservationIgnored private var pending: DispatchWorkItem?
    @ObservationIgnored private var hiddenAt: Date = .distantPast

    static var delay: TimeInterval {
        let ms = UserDefaults.standard.integer(forKey: "NSInitialToolTipDelay")
        return ms > 0 ? Double(ms) / 1000 : 1.0
    }
    static let warmWindow: TimeInterval = 0.5

    func enter(_ id: UUID, text: String, anchor: CGRect) {
        pending?.cancel()
        owner = id
        let warm = self.text != nil || Date().timeIntervalSince(hiddenAt) < Self.warmWindow
        if warm {
            show(text, anchor)
            return
        }
        latest = anchor
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                guard let self, self.owner == id else { return }
                self.show(text, self.latest)
            }
        }
        pending = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.delay, execute: work)
    }

    /// The control moved while its tooltip is pending or up.
    func move(_ id: UUID, anchor: CGRect) {
        guard owner == id else { return }
        latest = anchor
        if text != nil { self.anchor = anchor }
    }

    func leave(_ id: UUID) {
        guard owner == id else { return }
        hide()
    }

    func hide() {
        pending?.cancel()
        pending = nil
        owner = nil
        if text != nil { hiddenAt = Date() }
        withAnimation(.easeOut(duration: 0.08)) { text = nil }
    }

    private func show(_ text: String, _ anchor: CGRect) {
        self.anchor = anchor
        withAnimation(.easeOut(duration: 0.12)) { self.text = text }
    }
}

private struct TooltipCenterKey: EnvironmentKey {
    static let defaultValue: TooltipCenter? = nil
}

extension EnvironmentValues {
    var tooltips: TooltipCenter? {
        get { self[TooltipCenterKey.self] }
        set { self[TooltipCenterKey.self] = newValue }
    }
}

private struct Tip: ViewModifier {
    let text: String
    @State private var id = UUID()
    @State private var frame: CGRect = .zero
    @Environment(\.tooltips) private var center

    func body(content: Content) -> some View {
        content
            // Tracks moves as well as resizes: a GeometryReader only reruns on
            // a size change, so a control that fades in on hover (a row's
            // trailing buttons) kept the spot it first appeared at.
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .named(TooltipLayer.space)) } action: { f in
                frame = f
                center?.move(id, anchor: f)
            }
            .onHover { inside in
                guard let center else { return }
                if inside { center.enter(id, text: text, anchor: frame) } else { center.leave(id) }
            }
            .simultaneousGesture(TapGesture().onEnded { center?.hide() })
            .onDisappear { center?.leave(id) }
            .accessibilityHint(text)
    }
}

private struct TooltipLayer: ViewModifier {
    static let space = "cc-moss.tooltips"
    @State private var center = TooltipCenter()
    @State private var cardSize: CGSize = .zero

    func body(content: Content) -> some View {
        content
            .coordinateSpace(name: Self.space)
            .environment(\.tooltips, center)
            .overlay(alignment: .topLeading) {
                GeometryReader { g in
                    if let text = center.text {
                        TooltipCard(text: text)
                            .background(GeometryReader { c in
                                Color.clear.onAppear { cardSize = c.size }
                                    .onChange(of: c.size) { _, s in cardSize = s }
                            })
                            .offset(position(in: g.size))
                            .transition(.opacity)
                    }
                }
                .allowsHitTesting(false)
            }
    }

    /// Centred under the control; above it when there is no room below;
    /// always inside the window with a small margin.
    private func position(in bounds: CGSize) -> CGSize {
        let a = center.anchor
        let margin: CGFloat = 8
        var x = a.midX - cardSize.width / 2
        var y = a.maxY + 6
        if y + cardSize.height > bounds.height - margin { y = a.minY - cardSize.height - 6 }
        x = min(max(x, margin), bounds.width - cardSize.width - margin)
        return CGSize(width: x, height: y)
    }
}

/// The card: small type, the panel's raised surface, a hairline and a soft
/// shadow; long text wraps at 240 pt.
private struct TooltipCard: View {
    let text: String
    @Environment(\.colorScheme) private var scheme

    /// One width for the text and its card: the text's natural width, at
    /// most 240 pt (measured, so wrapping and background always agree).
    private var width: CGFloat {
        let font = NSFont.systemFont(ofSize: 12)
        let natural = (text as NSString).size(withAttributes: [.font: font]).width
        return min(ceil(natural) + 1, 240)
    }

    var body: some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(.primary)
            .lineSpacing(2)
            .multilineTextAlignment(.leading)
            .frame(width: width, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Theme.raised(scheme)))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Theme.hairline(scheme)))
            .shadow(color: .black.opacity(scheme == .dark ? 0.35 : 0.10), radius: 5, y: 2)
            .fixedSize()
    }
}
