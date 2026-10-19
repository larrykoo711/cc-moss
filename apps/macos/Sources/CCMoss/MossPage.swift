import MossKit
import SwiftUI

// The panel's "Moss" page (T2.3): how Moss is doing at a glance, then a
// conversation with it. Moss's abilities come first (user 2026-10-19): the
// bottom bar offers them as buttons, and typing is one click away.

struct MossPage: View {
    @Bindable var model: AppModel
    /// Settings › Moss, where Moss is switched on.
    let settings: () -> Void
    /// The panel's own ⌘ shortcuts and Escape, while the text field has the keyboard.
    var keys: (KeyPress) -> KeyPress.Result = { _ in .ignored }
    @Environment(\.colorScheme) private var scheme

    private var desk: MossDesk { model.mossDesk }

    var body: some View {
        VStack(spacing: 0) {
            MossStatusBar(model: model, settings: settings)
            Rectangle().fill(Theme.hairline(scheme)).frame(height: 1)
            if desk.messages.isEmpty {
                MossIntro(enabled: desk.canTalk, muted: desk.activity == .off) { model.doMoss($0) }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                MossConversation(model: model)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            MossInputBar(model: model, keys: keys)
        }
        .animation(.easeOut(duration: 0.18), value: desk.messages.isEmpty)
        .task(id: model.panelShown) {
            // Moss thinking doesn't touch the state folder: look again every
            // few seconds while the page is on screen.
            while !Task.isCancelled && model.mossOnScreen {
                try? await Task.sleep(for: .seconds(3))
                if model.mossOnScreen { await model.loadMoss() }
            }
        }
    }
}

// MARK: - Avatar and status

/// Moss's face: the ∞ mark on a brand-coloured rounded square.
struct MossAvatar: View {
    var size: CGFloat = 32
    /// Grey, while Moss is switched off.
    var muted = false

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)
                .fill(LinearGradient(colors: muted ? [Color(white: 0.62), Color(white: 0.52)]
                                                   : [Color(red: 1.0, green: 0.46, blue: 0.34), Theme.brand],
                                     startPoint: .top, endPoint: .bottom))
            Continuum()
                .stroke(style: StrokeStyle(lineWidth: max(1.4, size * 0.075), lineCap: .round, lineJoin: .round))
                .foregroundStyle(.white)
                .frame(width: size * 0.78, height: size * 0.78)
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

/// The colour of Moss's state, the same on the page and the rail.
enum MossTone {
    static func color(_ activity: MossActivity?) -> Color {
        switch activity {
        case .idle: .green
        case .thinking: Theme.brand
        case .waiting: Theme.amber
        case .offline: Theme.red
        case .off, nil: Color.secondary
        }
    }
}

/// A small status dot; it breathes while Moss thinks.
struct MossDot: View {
    let color: Color
    var breathing = false
    var size: CGFloat = 7
    @State private var dim = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: size, height: size)
            .opacity(breathing && dim ? 0.3 : 1)
            .scaleEffect(breathing && dim ? 0.8 : 1)
            .animation(breathing && !reduceMotion ? .easeInOut(duration: 1.1).repeatForever(autoreverses: true) : .default,
                       value: dim)
            .onAppear { dim = breathing && !reduceMotion }
            .onChange(of: breathing) { _, on in dim = on && !reduceMotion }
    }
}

/// Top of the page: who, how it's doing, and a way to its terminal (or to
/// Settings when it's off).
private struct MossStatusBar: View {
    @Bindable var model: AppModel
    let settings: () -> Void

    var body: some View {
        let status = model.mossDesk.status
        HStack(spacing: 10) {
            MossAvatar(size: 30, muted: status?.activity == .off)
            VStack(alignment: .leading, spacing: 1) {
                Text("Moss")
                    .font(.system(size: 14, weight: .semibold))
                HStack(spacing: 6) {
                    MossDot(color: MossTone.color(status?.activity), breathing: status?.activity == .thinking)
                    Text(status?.activityLine() ?? L.t("Checking…", "正在查看…"))
                        .font(.system(size: 11.5))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                .tip(status?.activityLine() ?? "")
            }
            Spacer(minLength: 8)
            if status?.activity == .off || (status?.activity == .offline && status?.daemonRunning == false) {
                QuietButton(title: L.t("Settings", "去设置"), icon: Icons.settings, action: settings)
            } else if model.canOpenMoss {
                QuietButton(title: L.t("Open in Terminal", "在终端打开"), icon: "terminal") {
                    _ = model.openMoss()
                }
                .tip(L.t("Moss's own terminal, where you can watch it work", "Moss 自己的终端，能看它在做什么"))
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 50)
        .accessibilityElement(children: .contain)
    }
}

// MARK: - Empty page

/// Nothing said yet: what Moss can do, as rows you can click.
private struct MossIntro: View {
    let enabled: Bool
    var muted = false
    let perform: (MossAction) -> Void

    var body: some View {
        VStack(spacing: 0) {
            Spacer(minLength: 12)
            MossAvatar(size: 44, muted: muted)
            Text(L.t("Moss keeps an eye on your sessions", "Moss 能帮你看着这些会话"))
                .font(.system(size: 14, weight: .semibold))
                .padding(.top, 12)
            Text(L.t("Pick one and Moss takes a look.", "点一下，Moss 就去看。"))
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .padding(.top, 4)
            VStack(spacing: 6) {
                ForEach(MossAction.all) { action in
                    MossActionRow(action: action, enabled: enabled) { perform(action) }
                }
            }
            .padding(.top, 18)
            .padding(.horizontal, 24)
            Spacer(minLength: 12)
        }
    }
}

private struct MossActionRow: View {
    let action: MossAction
    let enabled: Bool
    let perform: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: perform) {
            HStack(spacing: 10) {
                Image(systemName: action.symbol)
                    .font(.system(size: 11.5, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 26, height: 26)
                    .background(RoundedRectangle(cornerRadius: 7, style: .continuous)
                        .fill(Theme.brand.opacity(scheme == .dark ? 0.20 : 0.11)))
                VStack(alignment: .leading, spacing: 1) {
                    Text(action.title())
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(.primary)
                    Text(action.detail())
                        .font(.system(size: 11.5))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 6)
                Image(systemName: "arrow.up.right")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(hovering ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(.tertiary))
            }
            .padding(.horizontal, 10)
            .frame(height: 46)
            .background(RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(hovering ? Theme.selection(scheme) : Color.primary.opacity(scheme == .dark ? 0.03 : 0.02)))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.hairline(scheme)))
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.5)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside && enabled } }
        .tip(action.text())
    }
}

// MARK: - The conversation

private struct MossConversation: View {
    @Bindable var model: AppModel
    private static let bottom = "moss-bottom"

    var body: some View {
        let desk = model.mossDesk
        let messages = desk.messages
        let lastMoss = messages.last { $0.from == .moss }?.id
        let thinking = desk.activity == .thinking
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 10) {
                    ForEach(Array(messages.enumerated()), id: \.element.id) { i, m in
                        if let stamp = Self.stamp(m, after: i > 0 ? messages[i - 1] : nil) {
                            Text(stamp)
                                .font(.system(size: 10.5))
                                .foregroundStyle(.tertiary)
                                .padding(.top, i == 0 ? 0 : 4)
                        }
                        MossMessageView(model: model, message: m,
                                        buttons: m.id == lastMoss && desk.canTalk ? m.buttons : [])
                    }
                    if thinking {
                        MossThinking()
                            .transition(.opacity)
                    }
                    Color.clear.frame(height: 1).id(Self.bottom)
                }
                .padding(.horizontal, 14)
                .padding(.top, 14)
                .padding(.bottom, 6)
            }
            .scrollIndicators(.never)
            .defaultScrollAnchor(.bottom)
            .onAppear { proxy.scrollTo(Self.bottom, anchor: .bottom) }
            .onChange(of: messages.count) { _, _ in
                withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(Self.bottom, anchor: .bottom) }
            }
            .onChange(of: thinking) { _, _ in
                withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(Self.bottom, anchor: .bottom) }
            }
        }
        .animation(.easeOut(duration: 0.18), value: messages)
        .animation(.easeOut(duration: 0.18), value: thinking)
    }

    /// A quiet time line before the first message and after a long pause.
    static func stamp(_ m: MossMessage, after previous: MossMessage?) -> String? {
        guard let at = m.at else { return nil }
        if let before = previous?.at, at.timeIntervalSince(before) < 15 * 60 { return nil }
        let f = DateFormatter()
        f.locale = Locale(identifier: L.lang == .zh ? "zh_CN" : "en_US")
        f.setLocalizedDateFormatFromTemplate(Calendar.current.isDate(at, inSameDayAs: L.clock()) ? "jmm" : "MMMdjmm")
        return f.string(from: at)
    }
}

/// One message: yours on the right, Moss's on the left, a digest in the middle.
private struct MossMessageView: View {
    @Bindable var model: AppModel
    let message: MossMessage
    let buttons: [String]
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        switch message.from {
        case .events: digest
        case .moss: moss
        case .you, .other: yours
        }
    }

    private var digest: some View {
        HStack(spacing: 5) {
            Image(systemName: "dot.radiowaves.left.and.right")
                .font(.system(size: 9.5, weight: .semibold))
            Text(Self.untagged(message.text))
                .lineLimit(2)
                .multilineTextAlignment(.center)
        }
        .font(.system(size: 11))
        .foregroundStyle(.tertiary)
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 24)
        .tip(message.text)
    }

    /// "[动态] api-review 在等你" → "api-review 在等你": the line itself says it's a digest.
    static func untagged(_ text: String) -> String {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard t.hasPrefix("["), let end = t.firstIndex(of: "]"), t.distance(from: t.startIndex, to: end) <= 12 else { return t }
        return t[t.index(after: end)...].trimmingCharacters(in: .whitespaces)
    }

    private var moss: some View {
        HStack(alignment: .top, spacing: 8) {
            MossAvatar(size: 22)
                .padding(.top, 1)
            VStack(alignment: .leading, spacing: 6) {
                MossMarkdown(text: message.shownText(), quiet: message.fallback)
                    .padding(.horizontal, 11)
                    .padding(.vertical, 8)
                    .background(ChatBubble(mine: false).fill(Theme.raised(scheme)))
                    .overlay(ChatBubble(mine: false).stroke(Theme.hairline(scheme), lineWidth: 1))
                    .shadow(color: .black.opacity(scheme == .dark ? 0.0 : 0.04), radius: 2, y: 1)
                if !buttons.isEmpty {
                    MossChips(titles: buttons.map { ($0, nil) }) { i in model.sayToMoss(buttons[i]) }
                }
            }
            Spacer(minLength: 36)
        }
    }

    private var yours: some View {
        HStack(spacing: 0) {
            Spacer(minLength: 56)
            VStack(alignment: .trailing, spacing: 4) {
                Text(message.text)
                    .font(.system(size: 13))
                    .foregroundStyle(.primary)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 11)
                    .padding(.vertical, 8)
                    .background(ChatBubble(mine: true).fill(Theme.brand.opacity(scheme == .dark ? 0.26 : 0.13)))
                deliveryLine
            }
        }
    }

    @ViewBuilder private var deliveryLine: some View {
        switch message.delivery {
        case .sending:
            note(L.t("Sending…", "发送中…"))
        case .queued:
            // Queued with Moss free means it's being handed over right now.
            note(model.mossDesk.activity == .idle ? L.t("Sending…", "发送中…") : L.t("In line", "排队中"))
        case .failed:
            HStack(spacing: 4) {
                Image(systemName: "exclamationmark.circle.fill")
                    .font(.system(size: 10))
                    .foregroundStyle(Theme.red)
                Text(L.t("Didn't get through", "没送到"))
                    .foregroundStyle(.secondary)
                Text("·").foregroundStyle(.tertiary)
                if model.mossDesk.resent.contains(message.id) {
                    Text(L.t("Sent again", "已重发")).foregroundStyle(.tertiary)
                } else {
                    Button(L.t("Resend", "重发")) { model.resendToMoss(message.id) }
                        .buttonStyle(.plain)
                        .foregroundStyle(Theme.brand)
                        .fontWeight(.medium)
                        .handCursor()
                        .disabled(!model.mossDesk.canTalk)
                }
            }
            .font(.system(size: 11))
            .tip(message.error ?? L.t("Moss didn't get this one.", "这条没送到 Moss。"))
        default:
            EmptyView()
        }
    }

    private func note(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 11))
            .foregroundStyle(.tertiary)
    }
}

/// A chat bubble: rounded, with the corner nearest the speaker a little tighter.
private struct ChatBubble: Shape {
    let mine: Bool

    func path(in rect: CGRect) -> Path {
        let big: CGFloat = 13, small: CGFloat = 4
        return UnevenRoundedRectangle(topLeadingRadius: mine ? big : small, bottomLeadingRadius: big,
                                      bottomTrailingRadius: big, topTrailingRadius: mine ? small : big,
                                      style: .continuous).path(in: rect)
    }
}

/// Moss's words with basic Markdown: bold, italics, links, `code` inline,
/// and fenced code blocks.
private struct MossMarkdown: View {
    let text: String
    var quiet = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(Self.blocks(text).enumerated()), id: \.offset) { _, block in
                if block.code {
                    ScrollView(.horizontal) {
                        Text(block.text)
                            .font(.system(size: 11.5, design: .monospaced))
                            .textSelection(.enabled)
                            .padding(8)
                    }
                    .scrollIndicators(.never)
                    .background(RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(Color.primary.opacity(scheme == .dark ? 0.08 : 0.05)))
                } else {
                    Text(inline(block.text))
                        .font(.system(size: 13))
                        .foregroundStyle(quiet ? .secondary : .primary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .tint(Theme.brand)
                }
            }
        }
    }

    private func inline(_ raw: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        guard var out = try? AttributedString(markdown: raw, options: options) else { return AttributedString(raw) }
        for run in out.runs where run.inlinePresentationIntent?.contains(.code) == true {
            out[run.range].font = .system(size: 12, design: .monospaced)
            out[run.range].backgroundColor = Color.primary.opacity(scheme == .dark ? 0.12 : 0.07)
        }
        return out
    }

    struct Block {
        var text: String
        var code: Bool
    }

    /// Split on ``` fences; an unclosed fence runs to the end.
    static func blocks(_ text: String) -> [Block] {
        var out: [Block] = []
        var current: [Substring] = []
        var code = false
        func flush() {
            let joined = current.joined(separator: "\n")
            let body = code ? joined : joined.trimmingCharacters(in: .newlines)
            if !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { out.append(Block(text: body, code: code)) }
            current = []
        }
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                flush()
                code.toggle()
            } else {
                current.append(line)
            }
        }
        flush()
        return out.isEmpty ? [Block(text: text, code: false)] : out
    }
}

/// Moss is mid-turn: three dots where its answer will be.
private struct MossThinking: View {
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            MossAvatar(size: 22)
            TimelineView(.animation(minimumInterval: 1 / 20, paused: reduceMotion)) { context in
                let t = context.date.timeIntervalSinceReferenceDate
                HStack(spacing: 4) {
                    ForEach(0..<3) { i in
                        Circle()
                            .fill(Color.secondary)
                            .frame(width: 5, height: 5)
                            .opacity(reduceMotion ? 0.6 : 0.3 + 0.6 * max(0, sin((t * 2.2 - Double(i) * 0.35) * .pi)))
                    }
                }
                .padding(.horizontal, 12)
                .frame(height: 30)
                .background(ChatBubble(mine: false).fill(Theme.raised(scheme)))
                .overlay(ChatBubble(mine: false).stroke(Theme.hairline(scheme), lineWidth: 1))
            }
            Spacer()
        }
        .accessibilityLabel(L.t("Moss is thinking", "Moss 正在想"))
    }
}

// MARK: - Chips

/// Small buttons in a row: Moss's abilities, or the replies it suggests.
/// When they don't fit, they drop their icons, then scroll.
private struct MossChips: View {
    /// Title and an optional symbol.
    let titles: [(String, String?)]
    var enabled = true
    let tap: (Int) -> Void

    var body: some View {
        ViewThatFits(in: .horizontal) {
            row(icons: true)
            row(icons: false)
            row(icons: false, compact: true)
            ScrollView(.horizontal) { row(icons: false, compact: true).padding(.horizontal, 1) }
                .scrollIndicators(.never)
                .mask(LinearGradient(stops: [.init(color: .black, location: 0.88), .init(color: .clear, location: 1)],
                                     startPoint: .leading, endPoint: .trailing))
        }
    }

    private func row(icons: Bool, compact: Bool = false) -> some View {
        HStack(spacing: compact ? 4 : 6) {
            ForEach(Array(titles.enumerated()), id: \.offset) { i, item in
                MossChip(title: item.0, icon: icons ? item.1 : nil, compact: compact, enabled: enabled) { tap(i) }
            }
        }
        .fixedSize()
    }
}

private struct MossChip: View {
    let title: String
    let icon: String?
    var compact = false
    var enabled = true
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            HStack(spacing: 5) {
                if let icon {
                    Image(systemName: icon)
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(Theme.brand)
                }
                Text(title)
                    .font(.system(size: compact ? 11.5 : 12, weight: .medium))
                    .lineLimit(1)
                    .fixedSize()
            }
            .foregroundStyle(.primary)
            .padding(.horizontal, compact ? 7 : 10)
            .frame(height: 28)
            .background(Capsule().fill(hovering ? Theme.brand.opacity(scheme == .dark ? 0.20 : 0.10) : Theme.raised(scheme)))
            .overlay(Capsule().strokeBorder(hovering ? Theme.brand.opacity(0.45) : Theme.hairline(scheme)))
            .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.45)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside && enabled } }
    }
}

// MARK: - The bottom bar

/// Moss's abilities as buttons, plus a keyboard to type instead; or the
/// text field, with a way back to the buttons.
private struct MossInputBar: View {
    @Bindable var model: AppModel
    let keys: (KeyPress) -> KeyPress.Result
    @State private var text = ""
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    private var desk: MossDesk { model.mossDesk }
    private var blocked: String? { desk.status?.inputBlocked() }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let blocked {
                HStack(spacing: 5) {
                    Image(systemName: desk.activity == .off ? "moon.zzz" : "arrow.clockwise")
                        .font(.system(size: 10, weight: .semibold))
                    Text(blocked).lineLimit(2)
                }
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            }
            if model.mossTyping && blocked == nil {
                typing
            } else {
                actions
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.primary.opacity(scheme == .dark ? 0.02 : 0.015))
        .overlay(alignment: .top) { Rectangle().fill(Theme.hairline(scheme)).frame(height: 1) }
        .animation(.easeOut(duration: 0.15), value: model.mossTyping)
    }

    private var actions: some View {
        HStack(spacing: 6) {
            MossChips(titles: MossAction.all.map { ($0.title(), $0.symbol) }, enabled: blocked == nil) { i in
                model.doMoss(MossAction.all[i])
            }
            Spacer(minLength: 0)
            MossIconButton(symbol: "keyboard", tip: L.t("Type a message", "打字")) { model.mossTyping = true }
                .disabled(blocked != nil)
                .opacity(blocked == nil ? 1 : 0.45)
        }
    }

    private var typing: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField(L.t("Message Moss…", "和 Moss 说…"), text: $text, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .lineLimit(1...5)
                .focused($focused)
                .onKeyPress(.return, phases: .down) { press in
                    if press.modifiers.contains(.shift) {
                        text += "\n"
                    } else {
                        send()
                    }
                    return .handled
                }
                .onKeyPress(phases: .down) { press in keys(press) }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .frame(minHeight: 30)
                .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(Theme.raised(scheme)))
                .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .strokeBorder(focused ? Theme.brand.opacity(0.45) : Theme.hairline(scheme)))
            Button(action: send) {
                Image(systemName: "arrow.up")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 28, height: 28)
                    .background(Circle().fill(empty ? Color.secondary.opacity(0.35) : Theme.brand))
            }
            .buttonStyle(PressStyle())
            .focusable(false)
            .disabled(empty)
            .tip(L.t("Send  ↵", "发送  ↵"))
            .padding(.bottom, 1)
            MossIconButton(symbol: "square.grid.2x2", tip: L.t("Back to actions", "回到常用操作")) { model.mossTyping = false }
                .padding(.bottom, 1)
        }
        .onAppear { focused = true }
    }

    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private func send() {
        guard !empty else { return }
        model.sayToMoss(text)
        text = ""
    }
}

/// A borderless square icon button.
private struct MossIconButton: View {
    let symbol: String
    let tip: String
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(hovering ? .primary : .secondary)
                .frame(width: 28, height: 28)
                .background(RoundedRectangle(cornerRadius: 7, style: .continuous).fill(hovering ? Theme.selection(scheme) : .clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hovering = inside } }
        .tip(tip)
        .accessibilityLabel(tip)
    }
}
