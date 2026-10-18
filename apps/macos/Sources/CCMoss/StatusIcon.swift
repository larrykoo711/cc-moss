import AppKit
import MossKit
import SwiftUI

/// The Continuum mark: a figure eight with a small gap, the project's icon
/// (docs/assets/icon.svg) redrawn as a shape so it can animate later.
struct Continuum: Shape {
    /// The gap, as a fraction of the path, matching the icon's dash pattern.
    var gapStart = 0.695
    var gapEnd = 0.805
    /// How far the gap has travelled along the loop (0...1 wraps around):
    /// animating it makes the mark flow.
    var phase = 0.0

    var animatableData: Double {
        get { phase }
        set { phase = newValue }
    }

    func path(in rect: CGRect) -> Path {
        // The drawn part runs from the gap's end round to its start.
        Self.arc(in: rect, from: gapEnd + phase, to: gapStart + phase)
    }

    /// The whole loop: x = 32 + 27 cos t, y = 32 + 12.9 sin 2t in the icon's 64-unit box.
    static func loop(in rect: CGRect) -> Path {
        Path { p in
            let steps = 160
            for i in 0...steps {
                let t = Double(i) / Double(steps) * 2 * .pi
                let x = (32 + 27 * cos(t)) / 64
                let y = (32 + 12.9 * sin(2 * t)) / 64
                let point = CGPoint(x: rect.minX + x * rect.width, y: rect.minY + y * rect.height)
                i == 0 ? p.move(to: point) : p.addLine(to: point)
            }
        }
    }

    /// The stretch of the loop between two fractions, wrapping past the end.
    static func arc(in rect: CGRect, from: Double, to: Double) -> Path {
        let full = loop(in: rect)
        let a = from - from.rounded(.down)
        let b = to - to.rounded(.down)
        if a < b {
            return full.trimmedPath(from: a, to: b)
        }
        var out = full.trimmedPath(from: a, to: 1)
        out.addPath(full.trimmedPath(from: 0, to: b))
        return out
    }
}

/// The footer mark: flows while the pointer is on it; a click opens
/// cc-moss's own menu (new session, settings, quit), like an app menu.
struct LogoButton: View {
    let version: String
    /// Just the mark, when the action bar is short of room.
    var compact = false
    /// Its menu is open.
    var open = false
    let action: () -> Void
    @State private var hovering = false
    @State private var start = Date()

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                TimelineView(.animation(paused: !hovering)) { context in
                    let t = context.date.timeIntervalSince(start)
                    Continuum(phase: hovering ? (t / 1.6).truncatingRemainder(dividingBy: 1) : 0)
                        .stroke(style: StrokeStyle(lineWidth: 1.9, lineCap: .round, lineJoin: .round))
                        .foregroundStyle(hovering ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(.secondary))
                        .frame(width: 20, height: 20)
                }
                if !compact {
                    Text("cc-moss \(version)")
                        .font(.system(size: 12))
                        .lineLimit(1)
                        .fixedSize()
                        .foregroundStyle(hovering ? .primary : .secondary)
                }
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 4)
            .background(RoundedRectangle(cornerRadius: 7).fill(hovering || open ? Color.primary.opacity(0.06) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in
            if inside { start = Date() }
            withAnimation(.easeOut(duration: 0.15)) { hovering = inside }
        }
        .tip(L.t("New session, settings, quit", "新会话、设置、退出"))
        .accessibilityLabel(L.t("cc-moss menu", "cc-moss 菜单"))
    }
}

/// A slight press-in, the click feedback every button here shares.
struct PressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.96 : 1)
            .opacity(configuration.isPressed ? 0.8 : 1)
            .animation(.easeOut(duration: 0.08), value: configuration.isPressed)
            .handCursor()
    }
}

extension View {
    /// A specific pointer while hovering, set through AppKit so controls
    /// that bring their own cursor rects (switches) don't override it.
    func cursor(_ cursor: NSCursor) -> some View {
        onContinuousHover { phase in
            switch phase {
            case .active: cursor.set()
            case .ended: NSCursor.arrow.set()
            }
        }
    }

    /// The pointing hand while the pointer is over something you can click.
    @ViewBuilder func handCursor() -> some View {
        if #available(macOS 15.0, *) {
            self.pointerStyle(.link)
        } else {
            self.onContinuousHover { phase in
                switch phase {
                case .active: NSCursor.pointingHand.set()
                case .ended: NSCursor.arrow.set()
                }
            }
        }
    }
}

/// A button inside the floating action pill: highlight on hover, press-in
/// on click, and its key caps light up when the shortcut fires.
struct PillButton<Label: View>: View {
    let action: () -> Void
    @ViewBuilder let label: () -> Label
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            // A capsule inside the capsule, the same inset all round, so the
            // hover shape follows the pill's curve instead of cutting into it.
            label()
                .padding(.horizontal, 9)
                .frame(height: 26)
                .background(
                    Capsule().fill(hovering ? Theme.selection(scheme) : .clear)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }
}
