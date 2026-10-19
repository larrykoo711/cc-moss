import AppKit
import MossKit
import SwiftUI

// Sessions named in Moss's words as mentions (user 2026-10-19): part of the
// sentence, in its own colour, marked by a light dotted underline and a
// small soft state dot; coloured only under the pointer. Click goes to
// the session; hovering a moment shows its card. SwiftUI's Text can't tell
// which words the pointer is over, so a paragraph with mentions is drawn by
// a read-only text view that can.

/// A mention the pointer rests on, and where it is in the window (top-left origin).
struct MentionHover: Equatable {
    var id: String
    var anchor: CGRect
}

extension AppModel {
    /// The names a bubble's words may mention: its own sessions first, then
    /// every other session (open, or closed in the last 30 days).
    func mentionNames(_ group: MossGroup) -> [MossMentions.Name] {
        let all = sessions + (closedPool ?? [])
        let byID = Dictionary(all.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let own = group.sessions.compactMap { byID[$0] }
        return (own + all).filter { !$0.isMoss && $0.name?.isEmpty == false }
            .map { MossMentions.Name($0.title, id: $0.id) }
    }

    /// The session a mention points at, open or recently closed.
    func mentioned(_ id: String) -> Session? {
        sessions.first { $0.id == id } ?? closedPool?.first { $0.id == id }
    }

    /// A click on a mention: the Sessions page, on that session, in view;
    /// its reply line open when it waits for you to type. A closed one: a
    /// search for its name, so the Closed section shows it.
    func goToSession(_ id: String) {
        mentionHover = nil
        mentionPointer.clear()
        mentionShow?.cancel()
        if let s = sessions.first(where: { $0.id == id }) {
            page = .sessions
            query = ""
            stateFilter = nil
            follow(s)
            if ReplyLine.opens(s) { openReply(s) }
        } else if let s = closedPool?.first(where: { $0.id == id }) {
            page = .sessions
            stateFilter = nil
            query = s.title
        }
    }

    /// The pointer came onto a mention: its card after a moment.
    func mentionEntered(_ id: String, anchor: CGRect) {
        mentionPointer.enter(id)
        mentionHide?.cancel()
        if mentionHover?.id == id {
            mentionHover?.anchor = anchor
            return
        }
        mentionShow?.cancel()
        mentionShow = Task { @MainActor in
            try? await Task.sleep(nanoseconds: 350_000_000)
            guard !Task.isCancelled else { return }
            mentionHover = MentionHover(id: id, anchor: anchor)
        }
    }

    /// The pointer left mention `id`. Only the one lit counts: another
    /// paragraph's view may already have lit the next one.
    func mentionLeft(_ id: String) {
        guard mentionPointer.leave(id) else { return }
        mentionShow?.cancel()
        hideMentionCard()
    }

    /// Close the card after a short grace, unless the pointer comes back
    /// onto the mention or the card.
    private func hideMentionCard() {
        mentionHide?.cancel()
        mentionHide = Task { @MainActor in
            try? await Task.sleep(nanoseconds: 250_000_000)
            guard !Task.isCancelled else { return }
            mentionHover = nil
        }
    }

    func mentionCardHovered(_ inside: Bool) {
        if inside { mentionHide?.cancel() } else if mentionPointer.id == nil { hideMentionCard() }
    }
}

/// The card of the mention the pointer rests on, floating over the page
/// (an overlay, not a popover: nothing moves, and screenshots show it).
struct MentionHoverLayer: View {
    @Bindable var model: AppModel
    static let width: CGFloat = 300

    var body: some View {
        GeometryReader { geo in
            if let hover = model.mentionHover, hover.anchor != .zero,
               let card = model.mossCard(hover.id), let session = card.session {
                let page = geo.frame(in: .global)
                let anchor = hover.anchor.offsetBy(dx: -page.minX, dy: -page.minY)
                // Below the name when there's room, else above; inside the page.
                let below = anchor.maxY + 110 < geo.size.height
                let x = min(max(anchor.minX - 12, 8), geo.size.width - Self.width - 8)
                MossSessionCardView(model: model, card: card, session: session, hover: true, key: "mention/\(hover.id)")
                    .frame(width: Self.width)
                    .onHover { model.mentionCardHovered($0) }
                    // 6 pt off the line, never over the name itself.
                    .alignmentGuide(.top) { d in below ? -(anchor.maxY + 6) : -(anchor.minY - 6 - d.height) }
                    .alignmentGuide(.leading) { _ in -x }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.12), value: model.mentionHover?.id)
    }
}

/// One paragraph of Moss's words that names sessions.
struct MentionText: NSViewRepresentable {
    @Bindable var model: AppModel
    let text: AttributedString
    var size: CGFloat = 13
    var weight: Font.Weight = .regular
    var secondary = false
    @Environment(\.colorScheme) private var scheme

    func makeNSView(context: Context) -> MentionTextView {
        let view = MentionTextView()
        view.onEnter = { [model] id, rect in model.mentionEntered(id, anchor: rect) }
        view.onLeave = { [model] id in model.mentionLeft(id) }
        view.onClick = { [model] id in model.goToSession(id) }
        return view
    }

    func updateNSView(_ view: MentionTextView, context: Context) {
        view.textStorage?.setAttributedString(attributed())
        view.wantsAnchor = model.mentionHover.flatMap { $0.anchor == .zero ? $0.id : nil }
        view.onAnchor = { [model] id, rect in
            if model.mentionHover?.id == id { model.mentionHover?.anchor = rect }
        }
        view.needsLayout = true
        view.reportAnchor()
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: MentionTextView, context: Context) -> CGSize? {
        let width = proposal.width ?? 400
        return CGSize(width: width, height: nsView.height(for: width))
    }

    private func attributed() -> NSAttributedString {
        let base = NSFont.systemFont(ofSize: size, weight: weight.ns)
        let ink: NSColor = secondary ? .secondaryLabelColor : .labelColor
        let out = NSMutableAttributedString()
        for run in text.runs {
            let piece = String(text[run.range].characters)
            var font = base
            let intent = run.inlinePresentationIntent ?? []
            var attrs: [NSAttributedString.Key: Any] = [.foregroundColor: ink]
            if intent.contains(.code) {
                font = .monospacedSystemFont(ofSize: size - 1, weight: .regular)
                attrs[.backgroundColor] = NSColor.labelColor.withAlphaComponent(scheme == .dark ? 0.12 : 0.07)
            } else {
                if intent.contains(.stronglyEmphasized) { font = .systemFont(ofSize: size, weight: .semibold) }
                if intent.contains(.emphasized) {
                    font = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask)
                }
            }
            attrs[.font] = font
            if let url = run.link, let id = MossMentions.id(of: url) {
                // A mention reads as part of the sentence: its own colour and
                // weight, a light dotted underline, a small soft state dot.
                // Under the pointer the name and a solid underline turn brand.
                let lit = model.mentionPointer.id == id
                let brand = NSColor(Theme.brand)
                let state = model.mentioned(id).map { Theme.color($0.needsYou ? .working : $0.state) } ?? .secondary
                let mention: [NSAttributedString.Key: Any] = [.mossMention: id]
                out.append(NSAttributedString(string: "●\u{2009}", attributes: mention.merging([
                    .font: NSFont.systemFont(ofSize: size * 0.45),
                    .baselineOffset: size * 0.22,
                    .foregroundColor: NSColor(state).withAlphaComponent(0.7),
                ]) { a, _ in a }))
                var name = attrs.merging(mention) { _, b in b }
                name[.underlineStyle] = lit ? NSUnderlineStyle.single.rawValue
                                            : (NSUnderlineStyle.single.rawValue | NSUnderlineStyle.patternDot.rawValue)
                name[.underlineColor] = lit ? brand : brand.withAlphaComponent(0.35)
                if lit { name[.foregroundColor] = brand }
                out.append(NSAttributedString(string: piece, attributes: name))
            } else {
                if let url = run.link {
                    attrs[.link] = url
                }
                out.append(NSAttributedString(string: piece, attributes: attrs))
            }
        }
        return out
    }
}

extension NSAttributedString.Key {
    /// The slot id of the session a run of text mentions.
    static let mossMention = NSAttributedString.Key("ccMossMention")
}

private extension Font.Weight {
    var ns: NSFont.Weight {
        switch self {
        case .semibold: .semibold
        case .bold: .bold
        case .medium: .medium
        default: .regular
        }
    }
}

/// Read-only, selectable text that knows which mention the pointer is on.
final class MentionTextView: NSTextView {
    var onEnter: (String, CGRect) -> Void = { _, _ in }
    var onLeave: (String) -> Void = { _ in }
    var onClick: (String) -> Void = { _ in }
    /// Screenshots: a hover card asked for this mention; say where it is.
    var wantsAnchor: String?
    var onAnchor: (String, CGRect) -> Void = { _, _ in }
    private var current: String?

    init() {
        let storage = NSTextStorage()
        let layout = NSLayoutManager()
        storage.addLayoutManager(layout)
        let container = NSTextContainer(size: NSSize(width: 400, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = true
        container.lineFragmentPadding = 0
        layout.addTextContainer(container)
        super.init(frame: .zero, textContainer: container)
        isEditable = false
        isSelectable = true
        drawsBackground = false
        textContainerInset = .zero
        isVerticallyResizable = false
        isHorizontallyResizable = false
        linkTextAttributes = [.foregroundColor: NSColor(Theme.brand), .cursor: NSCursor.pointingHand]
    }

    override init(frame: NSRect, textContainer: NSTextContainer?) {
        super.init(frame: frame, textContainer: textContainer)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func height(for width: CGFloat) -> CGFloat {
        guard let container = textContainer, let layout = layoutManager else { return 0 }
        container.containerSize = NSSize(width: width, height: .greatestFiniteMagnitude)
        layout.ensureLayout(for: container)
        return ceil(layout.usedRect(for: container).height)
    }

    /// Screenshots: once it's laid out in a window, say where the mention is.
    /// Right away, not later: the screenshot runs inside one main-actor task.
    func reportAnchor() {
        guard let id = wantsAnchor, let rect = windowRect(of: id), rect.width > 0 else { return }
        wantsAnchor = nil
        onAnchor(id, rect)
    }

    override func layout() {
        super.layout()
        reportAnchor()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        reportAnchor()
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        // Its own bounds, not `.inVisibleRect`: with clipping off (the
        // default since macOS 14) the visible rect reaches past the view, so
        // every paragraph's area covered its neighbours and a view the
        // pointer wasn't on said "left", switching off the mention that was
        // just lit in the next paragraph.
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow],
                                       owner: self))
    }

    override func mouseMoved(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let id = bounds.contains(point) ? mention(at: point) : nil
        if bounds.contains(point) {
            if id != nil { NSCursor.pointingHand.set() } else { NSCursor.iBeam.set() }
        }
        move(to: id)
    }

    override func mouseExited(with event: NSEvent) {
        move(to: nil)
    }

    private func move(to id: String?) {
        guard id != current else { return }
        if let left = current { onLeave(left) }
        current = id
        if let id, let rect = windowRect(of: id) { onEnter(id, rect) }
    }

    override func mouseDown(with event: NSEvent) {
        if let id = mention(at: convert(event.locationInWindow, from: nil)) {
            onClick(id)
            return
        }
        super.mouseDown(with: event)
    }

    private func mention(at point: NSPoint) -> String? {
        guard let layout = layoutManager, let container = textContainer, let storage = textStorage,
              storage.length > 0 else { return nil }
        let glyph = layout.glyphIndex(for: point, in: container)
        let box = layout.boundingRect(forGlyphRange: NSRange(location: glyph, length: 1), in: container)
        guard box.insetBy(dx: -1, dy: -1).contains(point) else { return nil }
        let index = layout.characterIndexForGlyph(at: glyph)
        guard index < storage.length else { return nil }
        return storage.attribute(.mossMention, at: index, effectiveRange: nil) as? String
    }

    /// The mention's first line box, in window coordinates from the top left.
    private func windowRect(of id: String) -> CGRect? {
        guard let layout = layoutManager, let container = textContainer, let storage = textStorage,
              let window, let content = window.contentView else { return nil }
        var found: NSRange?
        storage.enumerateAttribute(.mossMention, in: NSRange(location: 0, length: storage.length)) { value, range, stop in
            if value as? String == id { found = range; stop.pointee = true }
        }
        guard let range = found else { return nil }
        let glyphs = layout.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
        let local = layout.boundingRect(forGlyphRange: glyphs, in: container)
        let inWindow = convert(local, to: nil)
        return CGRect(x: inWindow.minX, y: content.bounds.height - inWindow.maxY, width: inWindow.width, height: inWindow.height)
    }
}
