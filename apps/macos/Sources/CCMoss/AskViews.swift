import MossKit
import SwiftUI

// Answering a session from the panel or Mo (T1.4). A wrong answer sends the
// session the wrong way, so a card always says which session is asking and
// shows the whole question; options read exactly as the session wrote them.

/// The kind's tile, as on a session row that needs you.
struct AskTile: View {
    let kind: Ask.Kind
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 6).fill(Theme.brand.opacity(scheme == .dark ? 0.22 : 0.14))
            Image(systemName: kind == .permission ? Icons.needsYou : "questionmark.bubble")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Theme.brand)
        }
        .frame(width: 22, height: 22)
        .accessibilityHidden(true)
    }
}

/// The question and its answers, shared by the panel's card and Mo's bubble.
/// A single question ends with "Type something", as Claude Code's own list
/// does: picked, the row becomes a field. `inBubble`: Mo's bubble, which
/// takes the keyboard only while that field is open.
struct AskCardBody: View {
    let card: AskCard
    let sessionTitle: String
    var inBubble = false
    let act: ((inout AskCard) -> AskReply?) -> Void
    @FocusState private var fieldFocused: Bool
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            switch card.phase {
            case let .finished(outcome):
                OutcomeLine(outcome: outcome, title: sessionTitle)
            case .choosing, .submitting:
                if card.ask.kind == .permission {
                    permission
                } else if card.reviewing {
                    review
                } else if let q = card.question {
                    question(q)
                }
                if card.submitting {
                    HStack(spacing: 6) {
                        ProgressView().controlSize(.small).scaleEffect(0.7).frame(width: 14, height: 14)
                        Text(L.t("Handing it to \(sessionTitle)…", "正在交给 \(sessionTitle)…"))
                    }
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                }
                if let problem = card.problem {
                    Label(problem, systemImage: Icons.warning)
                        .font(.system(size: 12))
                        .foregroundStyle(Theme.amber)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .disabled(card.submitting)
    }

    // MARK: Questions

    @ViewBuilder private func question(_ q: Ask.Question) -> some View {
        if let header = q.header {
            Text(header)
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(Theme.brand)
                .padding(.horizontal, 6)
                .frame(height: 18)
                .background(Capsule().fill(Theme.brand.opacity(scheme == .dark ? 0.18 : 0.10)))
        }
        Text(q.text)
            .font(.system(size: 13.5, weight: .medium))
            .fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled)
            .padding(.bottom, 2)
        if q.multiSelect {
            Text(L.t("Pick any, then Done.", "可以多选，选好点「答好了」。"))
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
        }
        VStack(spacing: 6) {
            ForEach(Array(q.options.enumerated()), id: \.offset) { _, option in
                OptionButton(option: option, multi: q.multiSelect, on: card.picked(option.label)) {
                    act { $0.choose(option.label) }
                }
                .clickTarget(option.label) { act { $0.choose(option.label) } }
            }
            if card.offersOwnWords {
                if card.writing {
                    OwnWordsField(text: card.draft, focused: $fieldFocused, inBubble: inBubble,
                                  // One of several: these words answer it, then on to the next.
                                  sendTitle: card.several ? L.t("Next", "下一题") : L.t("Send", "发送"),
                                  change: { text in act { $0.draft = text; return nil } },
                                  send: { act { $0.sendText() } },
                                  cancel: { act { $0.stopWriting(); return nil } })
                } else {
                    OptionButton(option: Ask.Option(label: L.t("Type something", "自己写一个回答")), pencil: true) {
                        act { $0.startWriting(); return nil }
                    }
                    .clickTarget("type something") { act { $0.startWriting(); return nil } }
                }
            }
        }
        if q.multiSelect || card.step > 0 {
            questionButtons(q)
        }
    }

    private func questionButtons(_ q: Ask.Question) -> some View {
        HStack(spacing: 6) {
            if q.multiSelect {
                SoftButton(title: L.t("Done", "答好了"), icon: "checkmark", prominent: true) { act { $0.confirm() } }
                    .clickTarget("done") { act { $0.confirm() } }
                    .disabled(!q.options.contains { card.picked($0.label) })
                    .focusable(false)
            }
            if card.step > 0 {
                SoftButton(title: L.t("Back", "上一题"), icon: "chevron.left") { act { $0.back(); return nil } }
                    .focusable(false)
            }
        }
    }

    private var review: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(L.t("Check your answers, then send them.", "看一眼答案，没问题就交给它。"))
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(card.ask.questions.enumerated()), id: \.offset) { i, q in
                    ReviewRow(index: i, question: q,
                              answer: card.typed(i).map { L.t("“\($0)”", "「\($0)」") }
                                  ?? card.picks[i].map { OptionLabel.split($0).title }.joined(separator: L.t(", ", "、")),
                              typed: card.typed(i) != nil) {
                        act { $0.revisit(i); return nil }
                    }
                }
            }
            HStack(spacing: 6) {
                SoftButton(title: L.t("Send", "交给它"), icon: "paperplane", prominent: true) { act { $0.sendAll() } }
                    .clickTarget("send all") { act { $0.sendAll() } }
                    .focusable(false)
                SoftButton(title: L.t("Back", "上一题"), icon: "chevron.left") { act { $0.back(); return nil } }
                    .focusable(false)
            }
            .padding(.top, 2)
        }
    }

    // MARK: Permission

    private var permission: some View {
        VStack(alignment: .leading, spacing: 8) {
            let tool = card.ask.tool ?? Ask.Tool(name: "", preview: "")
            Text(L.t("Claude wants to use \(tool.name):", "Claude 想用 \(tool.name)："))
                .font(.system(size: 13))
            if !tool.preview.isEmpty {
                Text(tool.preview)
                    .font(.system(size: 12, design: .monospaced))
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Color.primary.opacity(0.06)))
            }
            HStack(spacing: 6) {
                SoftButton(title: L.t("Allow", "允许"), icon: "checkmark", prominent: true) { act { $0.allow() } }
                    .clickTarget("allow") { act { $0.allow() } }
                    .focusable(false)
                SoftButton(title: L.t("Don't Allow", "不允许"), icon: "xmark") { act { $0.deny() } }
                    .clickTarget("deny") { act { $0.deny() } }
                    .focusable(false)
            }
        }
    }
}

/// One answer: the session's label, its description underneath. A mark on
/// the left says how it picks (a ring for one, a box for several); under the
/// pointer the row takes the brand tint, so it's clear what a click will do.
struct OptionButton: View {
    let option: Ask.Option
    var multi = false
    var on = false
    /// "Type something": a pencil instead of the ring.
    var pencil = false
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme
    @Environment(\.isEnabled) private var enabled

    /// `--render … --hover-option <label>`: draw that option under the pointer.
    @MainActor static var renderHovered: String?

    private var lit: Bool { (hovering || Self.renderHovered == option.label) && enabled }

    var body: some View {
        Button(action: action) {
            HStack(alignment: .top, spacing: 10) {
                mark.padding(.top, 1)
                VStack(alignment: .leading, spacing: 2) {
                    let label = OptionLabel.split(option.label)
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(label.title)
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(.primary)
                            .fixedSize(horizontal: false, vertical: true)
                        if label.recommended {
                            // Claude's "(Recommended)", as a small tag.
                            Text(L.t("Recommended", "推荐"))
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(Theme.brand)
                                .padding(.horizontal, 5)
                                .padding(.vertical, 1)
                                .background(Capsule().fill(Theme.brand.opacity(scheme == .dark ? 0.2 : 0.1)))
                        }
                    }
                    if let d = option.description {
                        Text(d)
                            .font(.system(size: 11.5))
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 0)
                if !multi {
                    Image(systemName: "arrow.right")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(Theme.brand)
                        .opacity(lit ? 1 : 0)
                        .offset(x: lit ? 0 : -4)
                        .padding(.top, 3)
                        .accessibilityHidden(true)
                }
            }
            .multilineTextAlignment(.leading)
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(border))
            .contentShape(Rectangle())
            .opacity(enabled ? 1 : 0.55)
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
        .handCursor()
        .accessibilityLabel(option.label)
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private var fill: Color {
        if on { return Theme.brand.opacity(scheme == .dark ? 0.20 : 0.10) }
        if lit { return Theme.brand.opacity(scheme == .dark ? 0.14 : 0.07) }
        return Color.primary.opacity(scheme == .dark ? 0.04 : 0.025)
    }

    private var border: Color {
        if on { return Theme.brand.opacity(0.55) }
        if lit { return Theme.brand.opacity(0.40) }
        return Theme.hairline(scheme)
    }

    @ViewBuilder private var mark: some View {
        if pencil {
            Image(systemName: "pencil")
                .font(.system(size: 11.5, weight: .semibold))
                .foregroundStyle(lit ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(.secondary))
                .frame(width: 14, height: 14)
        } else if multi {
            Image(systemName: on ? "checkmark.square.fill" : "square")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(on || lit ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(.tertiary))
                .frame(width: 14, height: 14)
        } else {
            Circle()
                .strokeBorder(lit ? Theme.brand : Color.secondary.opacity(0.45), lineWidth: 1.5)
                .background(Circle().fill(Theme.brand).padding(3.5).opacity(lit ? 1 : 0))
                .frame(width: 14, height: 14)
        }
    }
}

/// "Type something", picked: the row is a field now, in the options' own
/// shape. At least two lines, growing with the text up to five, then it
/// scrolls. Return makes a new line, ⌘↩ sends, Esc goes back to the answers.
private struct OwnWordsField: View {
    let text: String
    var focused: FocusState<Bool>.Binding
    /// In Mo's bubble: its window takes the keyboard while this is open.
    var inBubble = false
    var sendTitle: String = L.t("Send", "发送")
    let change: (String) -> Void
    let send: () -> Void
    let cancel: () -> Void
    @Environment(\.colorScheme) private var scheme

    static let lineHeight: CGFloat = 17
    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        VStack(alignment: .trailing, spacing: 6) {
            HStack(alignment: .top, spacing: 5) { // the editor insets its text 5 pt: lines up with the answers
                Image(systemName: "pencil")
                    .font(.system(size: 11.5, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 14, height: 14)
                    .padding(.top, 2)
                editor
            }
            .padding(.leading, 10)
            .padding(.trailing, 6)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Theme.brand.opacity(scheme == .dark ? 0.12 : 0.06)))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Theme.brand.opacity(0.55)))
            SoftButton(title: sendTitle, icon: "paperplane", prominent: true, shortcut: "⌘↩", action: send)
                .disabled(empty)
                .focusable(false)
                .clickTarget("send text", send)
        }
        .background(inBubble ? KeyboardWhileOpen() : nil)
        .onAppear {
            // After the window can take the keyboard.
            DispatchQueue.main.async { focused.wrappedValue = true }
        }
    }

    /// The text sizes the field: an invisible copy, two to five lines tall,
    /// with the editor laid over it (it scrolls past five).
    private var editor: some View {
        ZStack(alignment: .topLeading) {
            Text(text.isEmpty ? " " : text + (text.hasSuffix("\n") ? " " : ""))
                .font(.system(size: 13))
                .lineSpacing(0)
                .lineLimit(5)
                .frame(maxWidth: .infinity, minHeight: Self.lineHeight * 2, alignment: .topLeading)
                .padding(.horizontal, 5)
                .hidden()
            if text.isEmpty {
                Text(L.t("Type your answer…", "写下你的回答…"))
                    .font(.system(size: 13))
                    .foregroundStyle(.tertiary)
                    .padding(.horizontal, 5)
                    .allowsHitTesting(false)
            }
            TextEditor(text: Binding(get: { text }, set: change))
                .font(.system(size: 13))
                .scrollContentBackground(.hidden)
                .focused(focused)
                .onKeyPress(.return, phases: .down) { press in
                    guard press.modifiers.contains(.command) else { return .ignored }
                    send()
                    return .handled
                }
                .onKeyPress(.escape) {
                    cancel()
                    return .handled
                }
        }
        .frame(maxHeight: Self.lineHeight * 5)
    }
}

/// Mo's bubble never takes the keyboard, except while its "Type something"
/// field is open: then its panel may become key (non-activating, so the app
/// you're in stays in front) and gives the keyboard back when the field goes.
private struct KeyboardWhileOpen: NSViewRepresentable {
    func makeNSView(context: Context) -> KeyView { KeyView() }
    func updateNSView(_ view: KeyView, context: Context) {}
    static func dismantleNSView(_ view: KeyView, coordinator: ()) { view.release() }

    final class KeyView: NSView {
        private weak var panel: BubblePanel?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard let panel = window as? BubblePanel else { return }
            self.panel = panel
            panel.takesTyping = true
            panel.makeKey()
        }

        func release() {
            guard let panel else { return }
            panel.takesTyping = false
            if panel.isKeyWindow {
                // Hand the keyboard back without hiding the bubble.
                panel.orderOut(nil)
                panel.orderFrontRegardless()
            }
        }
    }
}

/// How the answer went, in one calm line.
struct OutcomeLine: View {
    let outcome: AskOutcome
    let title: String

    var body: some View {
        let (symbol, color, words): (String, Color, String) = switch outcome {
        case .answered:
            ("checkmark.circle.fill", .green, L.t("Answered. \(title) carries on.", "答好了，\(title) 会接着往下做。"))
        case .late:
            ("clock", Theme.amber, L.t("Too late: \(title) already moved on.", "晚了一步：\(title) 已经往下走了。"))
        case .notWaiting:
            ("clock", Theme.amber, L.t("\(title) isn't waiting on it right now; the answer is saved for when it asks again.",
                                       "\(title) 这会儿没在等，答案先记下了，它再问时会交过去。"))
        case let .failed(message):
            (Icons.warning, Theme.amber, message)
        }
        return Label {
            Text(words).fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: symbol).foregroundStyle(color)
        }
        .font(.system(size: 12))
        .foregroundStyle(.secondary)
    }
}

/// A session waiting for you to type: say something back without opening
/// its terminal (sent the way the panel's "Send a Message" does).
struct ReplyBox: View {
    @Bindable var model: AppModel
    let session: Session
    @State private var text = ""
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        HStack(spacing: 6) {
            TextField(L.t("Reply…", "回它一句…"), text: $text)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .focused($focused)
                .onSubmit(send)
                .padding(.horizontal, 8)
                .frame(height: 28)
                .background(RoundedRectangle(cornerRadius: 7, style: .continuous).fill(Theme.surface(scheme)))
                .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).strokeBorder(Theme.hairline(scheme)))
            SoftButton(title: L.t("Send", "发送"), icon: "paperplane", prominent: true, shortcut: "↵", action: send)
                .disabled(empty)
                .focusable(false)
        }
        .onAppear { focused = true }
    }

    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private func send() {
        guard !empty else { return }
        model.sendMessage(session, text: text, done: L.t("Sent to \(session.title).", "已发给 \(session.title)。"))
        text = ""
    }
}

/// Mo's message box detail area for a question: the question and its
/// answers, answered right there. Live: it reads the card from the model.
/// "Type something" takes the keyboard for the bubble while it's open.
struct AskBubbleDetail: View {
    let model: AppModel
    let askId: String

    var body: some View {
        if let card = model.askDesk.card(askId) {
            VStack(alignment: .leading, spacing: 6) {
                if card.several, !card.reviewing, card.phase == .choosing {
                    Text(L.t("\(card.step + 1) of \(card.count)", "第 \(card.step + 1) / \(card.count) 题"))
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }
                AskCardBody(card: card, sessionTitle: model.askTitle(card.ask), inBubble: true) { body in
                    model.act(on: askId, body)
                }
            }
        }
    }
}

/// `--render … --click x,y` only: where each answer control was drawn, so
/// the probe can press the one under a point without any mouse event.
/// Off in the app: nothing is recorded.
@MainActor final class ClickTargets {
    static let shared = ClickTargets()
    var enabled = false
    struct Target {
        var name: String
        var frame: CGRect
        var action: () -> Void
    }
    private var targets: [String: Target] = [:]

    func set(_ name: String, _ frame: CGRect, _ action: @escaping () -> Void) {
        targets[name] = Target(name: name, frame: frame, action: action)
    }

    /// The smallest control under `point` (points from the window's top-left).
    func target(at point: CGPoint) -> Target? {
        targets.values.filter { $0.frame.contains(point) }.min { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }
    }
}

extension View {
    /// Record this control for `--click` (render only).
    func clickTarget(_ name: String, _ action: @escaping () -> Void) -> some View {
        background(GeometryReader { g in
            Color.clear.onAppear {
                if ClickTargets.shared.enabled { ClickTargets.shared.set(name, g.frame(in: .global), action) }
            }
            .onChange(of: g.frame(in: .global)) { _, frame in
                if ClickTargets.shared.enabled { ClickTargets.shared.set(name, frame, action) }
            }
        })
    }
}

extension PetMessageText {
    /// A message whose question is answered in the bubble: the main button
    /// opens the panel's card rather than the terminal.
    func answeringHere() -> PetMessageText {
        var text = self
        text.primary = L.t("Open Panel", "打开面板")
        return text
    }
}

/// One answered question on the review step: what was asked, small, and
/// the answer as a quote under it. Click to change that answer.
private struct ReviewRow: View {
    let index: Int
    let question: Ask.Question
    let answer: String
    /// Answered in your own words: a quote, with a pencil.
    var typed = false
    let change: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: change) {
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 6) {
                    Text(question.header ?? L.t("Question \(index + 1)", "第 \(index + 1) 题"))
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(Theme.brand)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1.5)
                        .background(Capsule().fill(Theme.brand.opacity(scheme == .dark ? 0.18 : 0.10)))
                    Spacer(minLength: 0)
                    Text(L.t("Change", "改"))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .opacity(hovering ? 1 : 0)
                }
                Text(question.text)
                    .font(.system(size: 11.5))
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .truncationMode(.tail)
                    .fixedSize(horizontal: false, vertical: true)
                    .multilineTextAlignment(.leading)
                HStack(alignment: .top, spacing: 6) {
                    Image(systemName: typed ? "pencil" : "checkmark")
                        .font(.system(size: 9.5, weight: .bold))
                        .foregroundStyle(Theme.brand)
                        .padding(.top, 2.5)
                    Text(answer)
                        .font(.system(size: 12.5, weight: typed ? .regular : .medium))
                        .foregroundStyle(.primary)
                        .fixedSize(horizontal: false, vertical: true)
                        .multilineTextAlignment(.leading)
                }
                .padding(.leading, 9)
                .padding(.vertical, 3)
                .overlay(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 1.25).fill(Theme.brand.opacity(0.55)).frame(width: 2.5)
                }
            }
            .padding(9)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 9, style: .continuous)
                .fill(hovering ? Theme.brand.opacity(scheme == .dark ? 0.10 : 0.05) : Theme.raised(scheme)))
            .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous)
                .strokeBorder(hovering ? Theme.brand.opacity(0.35) : Theme.hairline(scheme)))
            .contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
        .tip(L.t("Change this answer", "改这一题的答案"))
    }
}
