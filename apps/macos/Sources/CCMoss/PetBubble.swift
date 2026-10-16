import AppKit
import MossKit
import Observation
import SwiftUI

// Mo's two ways of talking (DESIGN-pet §6.3, §6.5): a translucent puff with
// a catchphrase, and the message box. Both live in one small window that
// rides along with Mo's (a child window), sized to the bubble plus a margin
// for its shadow, so the shadow never meets the window's edge.

/// Where the tail goes, from `BubblePlacement`.
struct BubbleTail: Equatable {
    /// The tip, from the bubble's leading edge.
    var x: CGFloat = 40
    /// The bubble is left of Mo: the tail hooks right, toward it.
    var leansRight = true
    /// The bubble is below Mo: the tail is on the top edge.
    var onTop = false

    /// Room the tail takes outside the bubble's body.
    static let height: CGFloat = 10
}

/// The colour and symbol of a message's kind, as in the panel (A21).
enum BubbleStyle {
    static func tint(_ kind: AlertKind) -> Color {
        switch kind {
        case .needsYou: Theme.brand
        case .interrupted: Theme.amber
        case .restored: Color.green
        case .finished: Theme.amber
        }
    }

    static func symbol(_ kind: AlertKind) -> String {
        switch kind {
        case .needsYou: Icons.needsYou
        case .interrupted: Icons.state(.interrupted)
        case .restored: "checkmark.circle"
        case .finished: Icons.state(.waiting)
        }
    }

    static func primaryIcon(_ message: PetMessage) -> String {
        if message.alerts.count > 1 { return "rectangle.on.rectangle" }
        return message.action == .restore ? Icons.state(.interrupted) : Icons.open
    }

    static func secondaryIcon(_ kind: AlertKind) -> String {
        switch kind {
        case .needsYou, .finished: "clock"
        case .interrupted: "xmark"
        case .restored: "checkmark"
        }
    }
}

/// A rounded card with a short curved tail toward Mo, drawn as one outline
/// so the hairline runs round the tail instead of across its base.
struct PetBubbleShape: Shape {
    var radius: CGFloat = 14
    var tail = BubbleTail()
    /// Just the rounded body (the puff, whose tail is dots).
    var tailless = false

    func path(in rect: CGRect) -> Path {
        let h = tailless ? 0 : BubbleTail.height
        let body = tail.onTop
            ? CGRect(x: rect.minX, y: rect.minY + h, width: rect.width, height: rect.height - h)
            : CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: rect.height - h)
        let r = min(radius, body.height / 2, body.width / 2)
        let lean: CGFloat = tail.leansRight ? 1 : -1
        let tip = rect.minX + tail.x
        // The base sits a little away from the tip, so the tail hooks toward Mo.
        let base = tip - lean * 5

        var p = Path()
        p.move(to: CGPoint(x: body.minX + r, y: body.minY))
        if !tailless, tail.onTop {
            // Left to right along the top edge, out to the tip and back.
            Self.tail(&p, edge: body.minY, out: -1, from: base - 9, to: base + 9, tip: tip, h: h)
        }
        p.addLine(to: CGPoint(x: body.maxX - r, y: body.minY))
        p.addArc(tangent1End: CGPoint(x: body.maxX, y: body.minY), tangent2End: CGPoint(x: body.maxX, y: body.minY + r), radius: r)
        p.addLine(to: CGPoint(x: body.maxX, y: body.maxY - r))
        p.addArc(tangent1End: CGPoint(x: body.maxX, y: body.maxY), tangent2End: CGPoint(x: body.maxX - r, y: body.maxY), radius: r)
        if !tailless, !tail.onTop {
            // Right to left along the bottom edge.
            Self.tail(&p, edge: body.maxY, out: 1, from: base + 9, to: base - 9, tip: tip, h: h)
        }
        p.addLine(to: CGPoint(x: body.minX + r, y: body.maxY))
        p.addArc(tangent1End: CGPoint(x: body.minX, y: body.maxY), tangent2End: CGPoint(x: body.minX, y: body.maxY - r), radius: r)
        p.addLine(to: CGPoint(x: body.minX, y: body.minY + r))
        p.addArc(tangent1End: CGPoint(x: body.minX, y: body.minY), tangent2End: CGPoint(x: body.minX + r, y: body.minY), radius: r)
        p.closeSubpath()
        return p
    }

    /// Along the edge to the tail's first base point, out to the tip and
    /// back: each side leaves the edge tangentially (a soft fillet) and
    /// meets the tip almost straight, like a speech bubble's.
    private static func tail(_ p: inout Path, edge: CGFloat, out: CGFloat, from a: CGFloat, to b: CGFloat, tip: CGFloat, h: CGFloat) {
        let towardB: CGFloat = b > a ? 1 : -1
        p.addLine(to: CGPoint(x: a, y: edge))
        p.addCurve(to: CGPoint(x: tip, y: edge + out * h),
                   control1: CGPoint(x: a + towardB * 4, y: edge),
                   control2: CGPoint(x: tip - towardB * 1.5, y: edge + out * h * 0.55))
        p.addCurve(to: CGPoint(x: b, y: edge),
                   control1: CGPoint(x: tip + towardB * 1.5, y: edge + out * h * 0.55),
                   control2: CGPoint(x: b - towardB * 4, y: edge))
    }
}

// MARK: - The puff

/// A word or two, translucent, with two little dots trailing to Mo's head:
/// "something's up", not the message itself.
struct PetPuff: View {
    let text: String
    var tail = BubbleTail()
    var animate = true
    @State private var shown: Bool
    @Environment(\.colorScheme) private var scheme

    static let height: CGFloat = 28
    static let translucency = 0.82

    init(text: String, tail: BubbleTail = BubbleTail(), animate: Bool = true) {
        self.text = text
        self.tail = tail
        self.animate = animate
        _shown = State(initialValue: !animate)
    }

    var body: some View {
        let lean: CGFloat = tail.leansRight ? 1 : -1
        let h = BubbleTail.height
        Text(text)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(.primary.opacity(0.85))
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, 13)
            .frame(height: Self.height)
            .background(PetBubbleShape(tailless: true).fill(Theme.raised(scheme)))
            .overlay(PetBubbleShape(tailless: true).stroke(Theme.hairline(scheme), lineWidth: 1))
            .padding(tail.onTop ? .top : .bottom, h)
            // The thought dots: the bigger one by the puff, the small one by Mo.
            .overlay(alignment: .topLeading) {
                GeometryReader { g in
                    let edge = tail.onTop ? h : g.size.height - h
                    let out: CGFloat = tail.onTop ? -1 : 1
                    dot(5.5).position(x: tail.x - lean * 4, y: edge + out * 2.5)
                    dot(3.5).position(x: tail.x, y: edge + out * (h - 2))
                }
            }
            // Translucent as a whole, so fill, hairline and words fade together.
            .compositingGroup()
            .opacity(shown ? Self.translucency : 0)
            .offset(y: shown ? 0 : (tail.onTop ? -3 : 3))
            .onAppear {
                guard !shown else { return }
                withAnimation(.easeOut(duration: 0.28)) { shown = true }
            }
            .accessibilityElement()
            .accessibilityLabel(L.t("Mo: ", "小莫：") + text)
    }

    private func dot(_ d: CGFloat) -> some View {
        Circle()
            .fill(Theme.raised(scheme))
            .overlay(Circle().strokeBorder(Theme.hairline(scheme)))
            .frame(width: d, height: d)
    }
}

// MARK: - The message box

/// The four parts of §6.3: the title row (kind tile, what happened, close),
/// the body (at most two lines, never session content), the detail area
/// (004's approve / deny, not here yet) and the actions (the main one in colour).
struct PetMessageBox: View {
    let message: PetMessage
    let text: PetMessageText
    var tail = BubbleTail()
    var animate = true
    var primary: () -> Void = {}
    var later: () -> Void = {}
    @State private var shown: Bool
    @State private var closeHover = false
    @Environment(\.colorScheme) private var scheme

    static let width: CGFloat = 280

    init(message: PetMessage, text: PetMessageText, tail: BubbleTail = BubbleTail(), animate: Bool = true,
         primary: @escaping () -> Void = {}, later: @escaping () -> Void = {}) {
        self.message = message
        self.text = text
        self.tail = tail
        self.animate = animate
        self.primary = primary
        self.later = later
        _shown = State(initialValue: !animate)
    }

    private var tint: Color { BubbleStyle.tint(message.kind) }

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 8) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6, style: .continuous).fill(tint.opacity(scheme == .dark ? 0.22 : 0.14))
                    Image(systemName: BubbleStyle.symbol(message.kind))
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(tint)
                }
                .frame(width: 22, height: 22)
                .accessibilityHidden(true)
                Text(text.title)
                    .font(.system(size: 13, weight: .semibold))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .onTapGesture(perform: primary)
                    .handCursor()
                Spacer(minLength: 6)
                closeButton
            }
            Text(text.body)
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
                .frame(width: Self.width - 24 - 30, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture(perform: primary)
                .handCursor()
                .padding(.leading, 30)
            HStack(spacing: 6) {
                SoftButton(title: text.primary, icon: BubbleStyle.primaryIcon(message), prominent: true, action: primary)
                    .focusable(false)
                SoftButton(title: text.secondary, icon: BubbleStyle.secondaryIcon(message.kind), action: later)
                    .focusable(false)
            }
            .padding(.top, 3)
            .padding(.leading, 30)
        }
        .padding(.horizontal, 12)
        .padding(.top, 11)
        .padding(.bottom, 12)
        .frame(width: Self.width, alignment: .leading)
        .padding(tail.onTop ? .top : .bottom, BubbleTail.height)
        .background(PetBubbleShape(tail: tail).fill(Theme.raised(scheme)))
        .overlay(PetBubbleShape(tail: tail).stroke(Theme.hairline(scheme), lineWidth: 1))
        .shadow(color: .black.opacity(scheme == .dark ? 0.42 : 0.13), radius: 12, y: 4)
        .scaleEffect(shown || !animate ? 1 : 0.94, anchor: tipAnchor)
        .opacity(shown ? 1 : 0)
        .onAppear {
            guard !shown else { return }
            withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) { shown = true }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(text.title + ". " + text.body)
    }

    /// Grow out of the tail, toward Mo.
    private var tipAnchor: UnitPoint {
        UnitPoint(x: tail.x / Self.width, y: tail.onTop ? 0 : 1)
    }

    private var closeButton: some View {
        Button(action: later) {
            Image(systemName: "xmark")
                .font(.system(size: 9, weight: .bold))
                .foregroundStyle(closeHover ? .secondary : .tertiary)
                .frame(width: 20, height: 20)
                .background(Circle().fill(closeHover ? Theme.selection(scheme) : .clear))
                .contentShape(Circle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { closeHover = inside } }
        .tip(text.secondary)
        .accessibilityLabel(L.t("Close", "关闭"))
    }
}

// MARK: - The window

/// What the bubble window shows.
@MainActor @Observable
final class BubbleStage {
    enum Content {
        case none
        case puff(serial: Int, text: String)
        case box(serial: Int, message: PetMessage, text: PetMessageText, primary: () -> Void, later: () -> Void)
    }

    var content: Content = .none
    var tail = BubbleTail()
    var animate = true
}

struct PetBubbleRoot: View {
    let stage: BubbleStage

    var body: some View {
        Group {
            switch stage.content {
            case .none:
                Color.clear.frame(width: 1, height: 1)
            case let .puff(serial, text):
                PetPuff(text: text, tail: stage.tail, animate: stage.animate).id(serial)
            case let .box(serial, message, text, primary, later):
                PetMessageBox(message: message, text: text, tail: stage.tail, animate: stage.animate,
                              primary: primary, later: later).id(serial)
            }
        }
        .padding(PetBubbles.margin)
        .fixedSize()
        .tooltipLayer()
    }
}

/// Hosts the bubble: hover and clicks work although the window is never key
/// and the app is usually not active (a tracking area that is always on);
/// the margin round the bubble lets clicks through.
final class BubbleHost: NSHostingView<PetBubbleRoot> {
    var onPointer: ((Bool) -> Void)?
    /// The pointing hand over a box (everything in it does something); the arrow over a puff.
    var clickable = false
    private var area: NSTrackingArea?
    private var inside = false

    required init(rootView: PetBubbleRoot) {
        super.init(rootView: rootView)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private var bubbleRect: NSRect { bounds.insetBy(dx: PetBubbles.margin, dy: PetBubbles.margin) }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let area { removeTrackingArea(area) }
        let a = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .mouseMoved, .cursorUpdate, .activeAlways, .inVisibleRect],
                               owner: self, userInfo: nil)
        addTrackingArea(a)
        area = a
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let local = convert(point, from: superview)
        return bubbleRect.contains(local) ? super.hitTest(point) : nil
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseEntered(with event: NSEvent) {
        super.mouseEntered(with: event)
        track(event)
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        track(event)
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        set(false)
    }

    override func cursorUpdate(with event: NSEvent) {
        (clickable && inside ? NSCursor.pointingHand : NSCursor.arrow).set()
    }

    private func track(_ event: NSEvent) {
        set(bubbleRect.contains(convert(event.locationInWindow, from: nil)))
    }

    private func set(_ now: Bool) {
        guard now != inside else { return }
        inside = now
        (clickable && inside ? NSCursor.pointingHand : NSCursor.arrow).set()
        onPointer?(now)
    }

    func reset() {
        if inside { set(false) }
    }
}

/// The bubble's window: like Mo's, transparent, above other windows, never key.
final class BubblePanel: NSPanel {
    init() {
        super.init(contentRect: NSRect(x: 0, y: 0, width: 10, height: 10),
                   styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        isFloatingPanel = true
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
        backgroundColor = .clear
        isOpaque = false
        hasShadow = false // the bubble draws its own, inside the window
        becomesKeyOnlyIfNeeded = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        isMovable = false
        animationBehavior = .none
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

/// Where Mo is, for placing the bubble.
struct BubbleAnchor: Equatable {
    /// Just above Mo's antenna, where the tail points when the bubble is above.
    var head: CGPoint
    /// Just below Mo, for when the bubble has to go underneath.
    var feet: CGPoint
    var visible: CGRect
}

/// Shows, moves and hides the bubble window beside Mo's.
@MainActor
final class PetBubbles {
    static let margin: CGFloat = 28

    var onPointer: ((Bool) -> Void)?
    private let stage = BubbleStage()
    private let panel = BubblePanel()
    private let host: BubbleHost
    private var key: String?
    private var anchor: BubbleAnchor?
    private var serial = 0
    private var generation = 0

    init() {
        host = BubbleHost(rootView: PetBubbleRoot(stage: stage))
        panel.contentView = host
        host.onPointer = { [weak self] inside in self?.onPointer?(inside) }
    }

    var isShown: Bool { key != nil }

    func puff(_ text: String, id: Int, anchor: BubbleAnchor, parent: NSWindow, reduceMotion: Bool) {
        present(key: "puff-\(id)", anchor: anchor, parent: parent, reduceMotion: reduceMotion, clickable: false) {
            .puff(serial: $0, text: text)
        }
    }

    func box(_ message: PetMessage, text: PetMessageText, anchor: BubbleAnchor, parent: NSWindow, reduceMotion: Bool,
             primary: @escaping () -> Void, later: @escaping () -> Void) {
        let key = "box-\(message.kind.rawValue)-\(message.slots.joined(separator: ","))-\(text.title)-\(text.body)"
        present(key: key, anchor: anchor, parent: parent, reduceMotion: reduceMotion, clickable: true) {
            .box(serial: $0, message: message, text: text, primary: primary, later: later)
        }
    }

    private func present(key: String, anchor: BubbleAnchor, parent: NSWindow, reduceMotion: Bool, clickable: Bool,
                         content: (Int) -> BubbleStage.Content) {
        let fresh = key != self.key
        guard fresh || anchor != self.anchor || !panel.isVisible else { return }
        generation += 1
        if fresh {
            serial += 1
            stage.animate = !reduceMotion
            stage.content = content(serial)
            self.key = key
        }
        host.clickable = clickable
        layout(anchor, puff: !clickable)
        if panel.parent == nil {
            panel.alphaValue = 0
            parent.addChildWindow(panel, ordered: .above)
        }
        if panel.alphaValue < 1 {
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.15
                panel.animator().alphaValue = 1
            }
        }
    }

    private func layout(_ anchor: BubbleAnchor, puff: Bool) {
        self.anchor = anchor
        host.layoutSubtreeIfNeeded()
        let size = host.fittingSize
        let place = BubblePlacement.place(size: size, margin: Self.margin, inset: puff ? 12 : 34, corner: puff ? 4 : 14,
                                          above: anchor.head, below: anchor.feet, visible: anchor.visible)
        let tail = BubbleTail(x: place.tailX, leansRight: place.leansRight, onTop: place.tailOnTop)
        if stage.tail != tail { stage.tail = tail }
        panel.setFrame(place.frame, display: true)
    }

    /// Fade out and take the window away.
    func hide(animated: Bool = true) {
        guard key != nil else { return }
        key = nil
        anchor = nil
        host.reset()
        generation += 1
        let mine = generation
        let finish = { [weak self] in
            guard let self, self.generation == mine else { return }
            self.panel.parent?.removeChildWindow(self.panel)
            self.panel.orderOut(nil)
            self.stage.content = .none
        }
        guard animated, panel.isVisible else {
            finish()
            return
        }
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.25
            panel.animator().alphaValue = 0
        }, completionHandler: { MainActor.assumeIsolated { finish() } })
    }

    func close() {
        hide(animated: false)
    }
}
