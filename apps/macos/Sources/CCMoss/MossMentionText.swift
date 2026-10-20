import AppKit
import MossKit
import SwiftUI

// Sessions named in Moss's words as mentions (user 2026-10-19): each one a
// small badge in the sentence, its state dot inside; under the pointer it
// fills in brand, and after a moment its card shows. Click goes to the
// session. A paragraph with mentions is laid out by `MentionFlowLayout`:
// words and characters wrap like text, and each badge is a real button, so
// hover and clicks are SwiftUI's own (proven in this panel).

/// A mention the pointer rests on, and where it is in the window (top-left origin).
struct MentionHover: Equatable {
    var id: String
    var anchor: CGRect
}

extension AppModel {
    /// The names a bubble's words may mention: its own sessions first, then
    /// every other session (open, or closed in the last 30 days).
    func mentionNames(_ group: MossGroup) -> [MossMentions.Name] {
        let all = sessions + archived + (closedPool ?? [])
        let byID = Dictionary(all.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let own = group.sessions.compactMap { byID[$0] }
        return (own + all).filter { !$0.isMoss && $0.name?.isEmpty == false }
            .map { MossMentions.Name($0.title, id: $0.id) }
    }

    /// The session a mention points at, open or recently closed.
    func mentioned(_ id: String) -> Session? {
        sessions.first { $0.id == id } ?? archived.first { $0.id == id } ?? closedPool?.first { $0.id == id }
    }

    /// A click on a mention: the Sessions page, on that session, in view;
    /// its reply line open when it waits for you to type. A closed one: a
    /// search for its name, so the Closed section shows it.
    func goToSession(_ id: String) {
        resetMentionHover()
        if let s = sessions.first(where: { $0.id == id }) {
            page = .sessions
            query = ""
            stateFilter = nil
            follow(s)
            if ReplyLine.opens(s) { openReply(s) }
        } else if let s = archived.first(where: { $0.id == id }) ?? closedPool?.first(where: { $0.id == id }) {
            page = .sessions
            stateFilter = nil
            query = s.title
        }
    }

    /// The pointer came onto a mention: its card after a moment, or at once
    /// when another mention's card is already showing.
    func mentionEntered(_ id: String, anchor: CGRect) {
        mentionAnchors[id] = anchor
        apply(mentionMachine.entered(id))
        if mentionHover?.id == id { mentionHover?.anchor = anchor }
    }

    /// The pointer left mention `id` (or its badge went away).
    func mentionLeft(_ id: String) {
        apply(mentionMachine.left(id))
    }

    func mentionCardHovered(_ inside: Bool) {
        apply(mentionMachine.cardHovered(inside))
    }

    /// The page or the panel went, or a mention was clicked: nothing stays
    /// lit or open, and nothing on its way shows up later.
    func resetMentionHover() {
        mentionShow?.cancel()
        mentionHide?.cancel()
        mentionMachine.reset()
        mentionHover = nil
    }

    private func apply(_ effect: MentionHoverMachine.Effect) {
        switch effect {
        case .none:
            break
        case .keep:
            mentionHide?.cancel()
        case let .showNow(id):
            mentionShow?.cancel()
            mentionHide?.cancel()
            mentionHover = MentionHover(id: id, anchor: mentionAnchors[id] ?? .zero)
        case let .showLater(id):
            mentionShow?.cancel()
            mentionHide?.cancel()
            mentionShow = Task { @MainActor in
                try? await Task.sleep(nanoseconds: 350_000_000)
                guard !Task.isCancelled, mentionMachine.showFired(id) else { return }
                mentionHover = MentionHover(id: id, anchor: mentionAnchors[id] ?? .zero)
            }
        case .hideLater:
            mentionShow?.cancel()
            mentionHide?.cancel()
            mentionHide = Task { @MainActor in
                try? await Task.sleep(nanoseconds: 250_000_000)
                guard !Task.isCancelled else { return }
                mentionMachine.hideFired()
                if mentionMachine.card == nil { mentionHover = nil }
            }
        }
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

/// One paragraph of Moss's words that names sessions: its words and
/// characters flow and wrap like text, each mention a badge among them.
struct MentionParagraph: View {
    @Bindable var model: AppModel
    let text: AttributedString
    var size: CGFloat = 13
    var weight: Font.Weight = .regular
    var secondary = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        MentionFlowLayout(lineSpacing: 3) {
            let pieces = MentionFlow.pieces(text)
            let glued = MentionFlow.glued(pieces)
            ForEach(Array(pieces.enumerated()), id: \.offset) { i, piece in
                Group {
                switch piece {
                case let .text(run):
                    Text(styled(run))
                        .font(.system(size: size, weight: weight))
                        // Alone, "，" would be set half-width: say it's Chinese.
                        .typesettingLanguage(.init(languageCode: L.lang == .zh ? .chinese : .english))
                        .fixedSize()
                case let .mention(id, name):
                    MentionBadge(model: model, id: id, name: name, size: size, weight: weight)
                case .lineBreak:
                    Color.clear.frame(width: 0, height: 0).layoutValue(key: LineBreakKey.self, value: true)
                }
                }
                .layoutValue(key: GlueKey.self, value: glued[i])
            }
        }
        .foregroundStyle(secondary ? .secondary : .primary)
        .textSelection(.enabled)
    }

    private func styled(_ text: AttributedString) -> AttributedString {
        var out = text
        for run in out.runs {
            let intent = run.inlinePresentationIntent ?? []
            if intent.contains(.code) {
                out[run.range].font = .system(size: size - 1, design: .monospaced)
                out[run.range].backgroundColor = Color.primary.opacity(scheme == .dark ? 0.12 : 0.07)
            } else if intent.contains(.stronglyEmphasized) {
                out[run.range].font = .system(size: size, weight: .semibold)
            }
        }
        return out
    }
}

/// The words a mention's card must not cover: the bubble's text, in global
/// coordinates. Zero outside one.
private struct MentionBlockKey: EnvironmentKey {
    static let defaultValue: CGRect = .zero
}

extension EnvironmentValues {
    var mentionBlock: CGRect {
        get { self[MentionBlockKey.self] }
        set { self[MentionBlockKey.self] = newValue }
    }
}

private struct MentionBlockReader: ViewModifier {
    @State private var frame: CGRect = .zero

    func body(content: Content) -> some View {
        content
            .environment(\.mentionBlock, frame)
            .background(GeometryReader { g in
                Color.clear
                    .onAppear { frame = g.frame(in: .global) }
                    .onChange(of: g.frame(in: .global)) { _, new in frame = new }
            })
    }
}

extension View {
    /// Mentions inside place their card below (or above) this whole block.
    func mentionBlock() -> some View { modifier(MentionBlockReader()) }
}

/// A session's name as a badge: a soft pill with its state dot; filled in
/// brand under the pointer, where a card follows after a moment.
private struct MentionBadge: View {
    @Bindable var model: AppModel
    let id: String
    let name: String
    let size: CGFloat
    let weight: Font.Weight
    @State private var frame: CGRect = .zero
    @Environment(\.colorScheme) private var scheme
    @Environment(\.mentionBlock) private var block

    /// Where the card goes: across from the badge, below or above the whole
    /// block of words, so it never covers another mention.
    private var anchor: CGRect {
        guard block != .zero else { return frame }
        return CGRect(x: frame.minX, y: block.minY, width: frame.width, height: block.height)
    }

    private var lit: Bool { model.mentionMachine.lit == id }
    private var session: Session? { model.mentioned(id) }
    private var stateColor: Color {
        guard let s = session else { return .secondary }
        return s.needsYou ? Theme.brand : Theme.color(s.state)
    }

    var body: some View {
        Button { model.goToSession(id) } label: {
            HStack(spacing: 4) {
                Circle()
                    .fill(stateColor.opacity(lit ? 1 : 0.75))
                    .frame(width: 5, height: 5)
                Text(name)
                    .font(.system(size: size, weight: weight == .regular ? .medium : weight))
                    .foregroundStyle(lit ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(.primary))
            }
            .padding(.horizontal, 5)
            .padding(.vertical, 1.5)
            .background(
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(lit ? Theme.brand.opacity(scheme == .dark ? 0.26 : 0.17)
                              : (session == nil ? Color.primary : stateColor).opacity(scheme == .dark ? 0.13 : 0.08))
            )
            .contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .handCursor()
        .fixedSize()
        // In place of the spaces around the name (MentionFlow trims them).
        .padding(.horizontal, 3)
        .background(GeometryReader { g in
            Color.clear
                .onAppear { frame = g.frame(in: .global); reportAnchor() }
                .onChange(of: g.frame(in: .global)) { _, new in frame = new; followAnchor() }
        })
        .onHover { inside in
            if inside { model.mentionEntered(id, anchor: anchor) } else { model.mentionLeft(id) }
        }
        .onChange(of: block) { _, _ in followAnchor() }
        // Gone from the page (scrolled out of a lazy list, the reply folded):
        // it can't stay lit or keep its card.
        .onDisappear { model.mentionLeft(id) }
        .accessibilityLabel(name)
        .accessibilityHint(L.t("Go to this session", "去这个会话"))
        .animation(.easeOut(duration: 0.1), value: lit)
    }

    /// Screenshots: a card asked for before any hover needs to know where.
    private func reportAnchor() {
        if model.mentionHover?.id == id, model.mentionHover?.anchor == .zero { model.mentionHover?.anchor = anchor }
    }

    /// Its card is showing and the words moved (a scroll, a reflow, the
    /// block measured): the card follows.
    private func followAnchor() {
        if model.mentionHover?.id == id, model.mentionHover?.anchor != anchor, frame != .zero {
            model.mentionHover?.anchor = anchor
        }
    }
}

/// Marks the paragraph's own line breaks among the flow's pieces.
private struct LineBreakKey: LayoutValueKey {
    static let defaultValue = false
}

/// A piece that can't start a line: it wraps with the one before it.
private struct GlueKey: LayoutValueKey {
    static let defaultValue = false
}

/// Pieces laid left to right, wrapping when the next doesn't fit, each
/// line's pieces sharing a baseline: text that wraps like text, with badges.
struct MentionFlowLayout: Layout {
    var lineSpacing: CGFloat = 3

    private struct Line {
        var items: [(index: Int, x: CGFloat, size: CGSize, ascent: CGFloat)] = []
        var ascent: CGFloat = 0
        var descent: CGFloat = 0
    }

    private func lines(_ subviews: Subviews, width: CGFloat) -> [Line] {
        var out: [Line] = [Line()]
        var x: CGFloat = 0
        for (i, sub) in subviews.enumerated() {
            if sub[LineBreakKey.self] {
                out.append(Line())
                x = 0
                continue
            }
            let size = sub.sizeThatFits(.unspecified)
            let d = sub.dimensions(in: .unspecified)
            let ascent = d[VerticalAlignment.firstTextBaseline]
            if x > 0, x + size.width > width {
                // Closing punctuation takes the piece before it along.
                var carried: (index: Int, x: CGFloat, size: CGSize, ascent: CGFloat)?
                if sub[GlueKey.self], out[out.count - 1].items.count > 1, let last = out[out.count - 1].items.popLast() {
                    carried = last
                    out[out.count - 1].ascent = out[out.count - 1].items.map(\.ascent).max() ?? 0
                    out[out.count - 1].descent = out[out.count - 1].items.map { $0.size.height - $0.ascent }.max() ?? 0
                }
                out.append(Line())
                x = 0
                if let carried {
                    out[out.count - 1].items.append((carried.index, 0, carried.size, carried.ascent))
                    out[out.count - 1].ascent = carried.ascent
                    out[out.count - 1].descent = carried.size.height - carried.ascent
                    x = carried.size.width
                }
            }
            out[out.count - 1].items.append((i, x, size, ascent))
            out[out.count - 1].ascent = max(out[out.count - 1].ascent, ascent)
            out[out.count - 1].descent = max(out[out.count - 1].descent, size.height - ascent)
            x += size.width
        }
        return out
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        let all = lines(subviews, width: width)
        let height = all.reduce(0) { $0 + $1.ascent + $1.descent } + lineSpacing * CGFloat(max(all.count - 1, 0))
        let used = all.map { line in line.items.last.map { $0.x + $0.size.width } ?? 0 }.max() ?? 0
        return CGSize(width: proposal.width ?? used, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for line in lines(subviews, width: bounds.width) {
            for item in line.items {
                subviews[item.index].place(at: CGPoint(x: bounds.minX + item.x, y: y + line.ascent - item.ascent),
                                           proposal: ProposedViewSize(item.size))
            }
            y += line.ascent + line.descent + lineSpacing
        }
    }
}
