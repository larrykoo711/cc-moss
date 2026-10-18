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
/// `canWrite`: "Write my own" opens a field here (the panel); Mo's bubble
/// can't take typing, so there it is left out.
struct AskCardBody: View {
    let card: AskCard
    let sessionTitle: String
    var canWrite = true
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
        }
        if q.multiSelect || card.step > 0 || (canWrite && card.canWrite && !card.writing) {
            questionButtons(q)
        }
        if canWrite, card.canWrite, card.writing {
            OwnWordsField(text: card.draft, focused: $fieldFocused,
                          change: { text in act { $0.draft = text; return nil } },
                          send: { act { $0.sendText() } })
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
            if canWrite, card.canWrite, !card.writing {
                SoftButton(title: L.t("Write my own…", "自己写…"), icon: "square.and.pencil") {
                    act { $0.writing = true; return nil }
                    fieldFocused = true
                }
                .focusable(false)
            }
        }
    }

    private var review: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(L.t("Check your answers, then send them.", "看一眼答案，没问题就交给它。"))
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
            ForEach(Array(card.ask.questions.enumerated()), id: \.offset) { i, q in
                VStack(alignment: .leading, spacing: 1) {
                    Text(q.text)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(card.picks[i].joined(separator: L.t(", ", "、")))
                        .font(.system(size: 13, weight: .medium))
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            HStack(spacing: 6) {
                SoftButton(title: L.t("Send", "交给它"), icon: "paperplane", prominent: true) { act { $0.sendAll() } }
                    .clickTarget("send all") { act { $0.sendAll() } }
                    .focusable(false)
                SoftButton(title: L.t("Back", "上一题"), icon: "chevron.left") { act { $0.back(); return nil } }
                    .focusable(false)
            }
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
                    Text(option.label)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(.primary)
                        .fixedSize(horizontal: false, vertical: true)
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
        if multi {
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

/// "Write my own": Return sends.
private struct OwnWordsField: View {
    let text: String
    var focused: FocusState<Bool>.Binding
    let change: (String) -> Void
    let send: () -> Void
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        HStack(spacing: 6) {
            TextField(L.t("Your answer…", "你的回答…"), text: Binding(get: { text }, set: change))
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .focused(focused)
                .onSubmit(send)
                .padding(.horizontal, 8)
                .frame(height: 28)
                .background(RoundedRectangle(cornerRadius: 7, style: .continuous).fill(Theme.raised(scheme)))
                .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).strokeBorder(Theme.hairline(scheme)))
            SoftButton(title: L.t("Send", "发送"), icon: "paperplane", prominent: true, shortcut: "↵", action: send)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .focusable(false)
                .clickTarget("send text", send)
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
/// No typing here (Mo's bubble never takes the keyboard): "Open Panel" has
/// "Write my own".
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
                AskCardBody(card: card, sessionTitle: model.askTitle(card.ask), canWrite: false) { body in
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
