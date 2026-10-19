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
                    .safeAreaInset(edge: .bottom, spacing: 0) { MossGuessInset(model: model) }
            } else {
                MossConversation(model: model)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            MossInputBar(model: model, keys: keys)
        }
        .overlay(alignment: .bottomTrailing) {
            if model.mossQuickOpen {
                ZStack(alignment: .bottomTrailing) {
                    // A click anywhere else closes it, as a menu does.
                    Color.clear.contentShape(Rectangle()).onTapGesture { model.mossQuickOpen = false }
                    MossQuickMenu(model: model)
                        .padding(.trailing, 10)
                        .padding(.bottom, 54)
                }
                .transition(.opacity)
            }
        }
        .overlay { MentionHoverLayer(model: model) }
        .animation(.easeOut(duration: 0.12), value: model.mossQuickOpen)
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
                HStack(alignment: .top, spacing: 6) {
                    MossDot(color: status?.inputBusy == true && status?.activity.canTalk == true ? Theme.amber : MossTone.color(status?.activity),
                            breathing: status?.activity == .thinking && status?.inputBusy != true)
                        .padding(.top, 4.5)
                    // A long reason (stuck input, a failed start) takes a second line.
                    Text(status?.activityLine() ?? L.t("Checking…", "正在查看…"))
                        .font(.system(size: 11.5))
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .truncationMode(.tail)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .tip(status?.activityLine() ?? "")
            }
            Spacer(minLength: 8)
            if status?.activity == .off || (status?.activity == .offline && status?.daemonRunning == false) {
                QuietButton(title: L.t("Settings", "去设置"), icon: Icons.settings, action: settings)
            } else if model.canOpenMoss {
                QuietButton(title: L.t("Open in Terminal", "在终端打开"), icon: "terminal") {
                    model.openMoss()
                }
                .tip(L.t("Moss's own terminal, where you can watch it work", "Moss 自己的终端，能看它在做什么"))
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 7)
        .frame(minHeight: 50)
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
    private static let topID = "moss-top"

    /// Following the conversation: new messages scroll in. Only scrolling up
    /// yourself stops it; a tall reply pushing the end out of view does not
    /// (that pushed-out moment used to read as "you scrolled up", so a long
    /// reply arrived and the page stayed put).
    @State private var following = true
    /// Moss's messages that came while you were scrolled up.
    @State private var newBelow = 0
    /// When the conversation last grew, to tell that apart from scrolling.
    @State private var grewAt = Date.distantPast
    @State private var loadingEarlier = false

    var body: some View {
        let desk = model.mossDesk
        let messages = desk.messages
        let groups = MossThread.groups(messages)
        let lastMoss = messages.last { $0.from == .moss }?.id
        let waiting = desk.waiting
        let thinking = waiting != nil
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 10) {
                    top(messages, proxy: proxy)
                        .id(Self.topID)
                    ForEach(Array(groups.enumerated()), id: \.element.id) { i, group in
                        // Time lines go between groups only.
                        if let stamp = Self.stamp(group.first, after: i > 0 ? groups[i - 1].last : nil) {
                            Text(stamp)
                                .font(.system(size: 10.5))
                                .foregroundStyle(.tertiary)
                                .padding(.top, i == 0 ? 0 : 4)
                        }
                        MossMessageView(model: model, group: group,
                                        buttons: group.last.id == lastMoss && desk.canTalk ? group.last.buttons : [])
                    }
                    if let waiting {
                        MossThinking(waiting: waiting)
                            .transition(.opacity)
                    }
                    Color.clear.frame(height: 1).id(Self.bottom)
                        .onAppear {
                            guard model.demoNewBelow == nil else { return }
                            following = true
                            newBelow = 0
                        }
                        .onDisappear {
                            // Out of view because something grew: still following.
                            if Date().timeIntervalSince(grewAt) > 0.8 { following = false }
                        }
                }
                .padding(.horizontal, 14)
                .padding(.top, 14)
                .padding(.bottom, 6)
            }
            .scrollIndicators(.never)
            // Scrolled up: a way back to the newest, floating over the
            // conversation (above the guess), so nothing moves.
            .overlay(alignment: .bottomTrailing) {
                if !following {
                    JumpToLatest(count: newBelow) {
                        following = true
                        newBelow = 0
                        toEnd(proxy)
                    }
                    .padding(.trailing, 14)
                    .padding(.bottom, 10)
                    .transition(.opacity.combined(with: .scale(scale: 0.9)))
                }
            }
            .animation(.easeOut(duration: 0.18), value: following)
            // Moss's guess sits under the conversation, which ends above it:
            // it never covers the last reply or card (user 2026-10-19).
            .safeAreaInset(edge: .bottom, spacing: 0) { MossGuessInset(model: model) }
            .onChange(of: model.mossShownGuess != nil) { _, _ in
                grewAt = Date()
                if following { toEnd(proxy) }
            }
            .defaultScrollAnchor(model.mossStartAtTop ? .top : .bottom)
            .onAppear {
                if !model.mossStartAtTop { proxy.scrollTo(Self.bottom, anchor: .bottom) }
                // Screenshots: as if scrolled up while this many came in.
                if let n = model.demoNewBelow {
                    following = false
                    newBelow = n
                }
            }
            .onChange(of: messages.last?.id) { _, _ in
                // New at the end: follow it if you were following, or if it's yours.
                grewAt = Date()
                guard !loadingEarlier else { return }
                if messages.last?.from == .you { following = true }
                if following {
                    toEnd(proxy)
                } else if messages.last?.from == .moss {
                    newBelow += 1
                }
            }
            .onChange(of: thinking) { _, _ in
                grewAt = Date()
                if following { toEnd(proxy) }
            }
        }
        .animation(.easeOut(duration: 0.18), value: messages)
        .animation(.easeOut(duration: 0.18), value: thinking)
    }

    /// To the end, once now and once more after a tall reply (a drawn card,
    /// a folded block) has finished laying out.
    private func toEnd(_ proxy: ScrollViewProxy) {
        withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(Self.bottom, anchor: .bottom) }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            grewAt = Date()
            withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(Self.bottom, anchor: .bottom) }
        }
    }

    /// Above the oldest message: "Load earlier" while there may be more, or
    /// past 100, the way to the full record in the terminal.
    @ViewBuilder private func top(_ messages: [MossMessage], proxy: ScrollViewProxy) -> some View {
        switch model.mossTop {
        case .nothing:
            EmptyView()
        case .loadEarlier:
            Button {
                // Keep the message you were looking at where it is.
                let anchor = messages.first?.id
                loadingEarlier = true
                Task {
                    await model.loadEarlierMoss()
                    // The message may now share a bubble with older ones: find its group.
                    let groups = MossThread.groups(model.mossDesk.messages)
                    if let anchor, let target = groups.first(where: { $0.messages.contains { $0.id == anchor } })?.id {
                        proxy.scrollTo(target, anchor: .top)
                    }
                    loadingEarlier = false
                }
            } label: {
                HStack(spacing: 5) {
                    if loadingEarlier {
                        ProgressView().controlSize(.mini)
                    } else {
                        Text(Image(systemName: "arrow.up"))
                            .font(.system(size: 9.5, weight: .semibold))
                    }
                    Text(L.t("Load earlier", "加载更早的"))
                }
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 10)
                .frame(height: 24)
                .background(Capsule().fill(Color.primary.opacity(0.05)))
                .contentShape(Capsule())
            }
            .buttonStyle(PressStyle())
            .focusable(false)
            .disabled(loadingEarlier)
            .padding(.bottom, 2)
        case .terminal:
            VStack(spacing: 6) {
                Text(L.t("Earlier messages are in the terminal", "更早的记录在终端里"))
                    .font(.system(size: 11.5))
                    .foregroundStyle(.tertiary)
                if model.canOpenMoss {
                    QuietButton(title: L.t("Open in Terminal", "在终端打开"), icon: "terminal") { model.openMoss() }
                }
            }
            .padding(.bottom, 4)
        }
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

/// One group of messages (one, or several said in one go): yours on the
/// right, Moss's on the left, one bubble, each message its own paragraph.
private struct MossMessageView: View {
    @Bindable var model: AppModel
    let group: MossGroup
    let buttons: [String]
    @Environment(\.colorScheme) private var scheme

    /// The message that speaks for the group: the last one (its buttons, its
    /// delivery line); for a lone card or refusal, the only one.
    private var message: MossMessage { group.last }

    var body: some View {
        switch message.from {
        case .events: EmptyView() // digests are for Moss, not the conversation
        case .moss: moss
        case .you, .other: yours
        }
    }

    private var cards: [MossSessionCard] { model.mossCards(group) }
    private var mentionNames: [MossMentions.Name] { model.mentionNames(group) }
    private var allHandled: Bool { MossSessionCard.allHandled(cards) }

    private var moss: some View {
        HStack(alignment: .top, spacing: 8) {
            MossAvatar(size: 22)
                .padding(.top, 1)
            VStack(alignment: .leading, spacing: 6) {
                if let command = message.command, !message.refused {
                    MossTerminalCard(command: command, text: message.text,
                                     graphic: MossScreenCard.make(command: command, text: message.text),
                                     fold: (message.id, model))
                } else {
                    MossFoldable(id: group.id, model: model) {
                        VStack(alignment: .leading, spacing: 8) {
                            ForEach(group.messages) { m in
                                // All its sessions handled: Moss's words are history now.
                                MossRichText(text: m.shownText(), quiet: m.fallback || allHandled,
                                             mentions: m.fallback ? [] : mentionNames, model: model)
                            }
                        }
                        // A mention's card goes below (or above) all of these words,
                        // never over a line that holds another mention.
                        .mentionBlock()
                        .padding(.horizontal, 11)
                        .padding(.vertical, 8)
                    }
                        .background(ChatBubble(mine: false).fill(Theme.raised(scheme)))
                        .clipShape(ChatBubble(mine: false))
                        .overlay(ChatBubble(mine: false).stroke(Theme.hairline(scheme), lineWidth: 1))
                        .shadow(color: .black.opacity(scheme == .dark ? 0.0 : 0.04), radius: 2, y: 1)
                }
                // One stack for the whole bubble: every session its messages name.
                if !cards.isEmpty {
                    MossSessionCards(model: model, messageID: group.id, cards: cards)
                }
                if message.refused, model.canOpenMoss {
                    QuietButton(title: L.t("Open in Terminal", "在终端打开"), icon: "terminal") { model.openMoss() }
                }
                if !buttons.isEmpty {
                    MossChips(titles: buttons.map { ($0, nil) }, enabled: !model.mossDesk.answered.contains(message.id)) { i in
                        model.chooseForMoss(buttons[i], on: message.id)
                    }
                }
            }
            // A terminal screen gets the whole width; words leave room on the right.
            Spacer(minLength: message.command != nil && !message.refused ? 0 : 36)
        }
    }

    private var yours: some View {
        HStack(spacing: 0) {
            Spacer(minLength: 56)
            VStack(alignment: .trailing, spacing: 4) {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(group.messages) { m in
                        Text(m.text)
                            .font(.system(size: MossCommand.looksLikeCommand(m.text) ? 12.5 : 13,
                                          design: MossCommand.looksLikeCommand(m.text) ? .monospaced : .default))
                            .foregroundStyle(.primary)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
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

/// Moss's words as Markdown: headings, lists, quotes, rules, fenced code,
/// tables, and bold / italics / links / `code` inline.
private struct MossRichText: View {
    let text: String
    var quiet = false
    /// Sessions its words may name: each becomes a mention.
    var mentions: [MossMentions.Name] = []
    var model: AppModel?
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            ForEach(Array(MossMarkdown.blocks(text).enumerated()), id: \.offset) { _, block in
                view(for: block)
            }
        }
    }

    @ViewBuilder private func view(for block: MossBlock) -> some View {
        switch block {
        case let .paragraph(raw):
            words(raw)
        case let .heading(level, raw):
            words(raw, size: level <= 1 ? 15 : level == 2 ? 14 : 13, weight: .semibold)
                .padding(.top, 2)
        case let .list(_, items):
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(item.marker ?? (item.level == 0 ? "•" : "◦"))
                            .font(.system(size: 13, weight: item.marker == nil ? .bold : .medium))
                            .monospacedDigit()
                            .foregroundStyle(Theme.brand.opacity(quiet ? 0.6 : 0.9))
                            .frame(minWidth: item.marker == nil ? 8 : 16, alignment: .trailing)
                        words(item.text)
                    }
                    .padding(.leading, CGFloat(item.level) * 14)
                }
            }
        case let .quote(raw):
            words(raw, secondary: true)
                .padding(.leading, 10)
                .overlay(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 1.5).fill(Theme.brand.opacity(0.45)).frame(width: 3)
                }
        case let .code(body):
            ScrollView(.horizontal) {
                Text(body)
                    .font(.system(size: 11.5, design: .monospaced))
                    .textSelection(.enabled)
                    .padding(8)
            }
            .scrollIndicators(.never)
            .background(RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(Color.primary.opacity(scheme == .dark ? 0.08 : 0.05)))
        case let .table(header, rows):
            table(header: header, rows: rows)
        case .rule:
            Rectangle().fill(Theme.hairline(scheme)).frame(height: 1).padding(.vertical, 2)
        }
    }

    @ViewBuilder private func words(_ raw: String, size: CGFloat = 13, weight: Font.Weight = .regular,
                                    secondary: Bool = false) -> some View {
        let marked = MossMentions.mark(MossMarkdown.inline(raw), names: mentions)
        if let model, marked.runs.contains(where: { $0.link.flatMap(MossMentions.id(of:)) != nil }) {
            // Names a session: drawn so each name can be clicked and hovered.
            MentionParagraph(model: model, text: marked, size: size, weight: weight, secondary: quiet || secondary)
        } else {
            Text(styled(marked))
                .font(.system(size: size, weight: weight))
                .foregroundStyle(quiet || secondary ? .secondary : .primary)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .tint(Theme.brand)
        }
    }

    private func table(header: [String], rows: [[String]]) -> some View {
        let columns = max(header.count, rows.map(\.count).max() ?? 0)
        func cell(_ row: [String], _ i: Int) -> String { i < row.count ? row[i] : "" }
        return ScrollView(.horizontal) {
            Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 5) {
                GridRow {
                    ForEach(0..<columns, id: \.self) { i in
                        words(cell(header, i), size: 12, weight: .semibold)
                    }
                }
                Rectangle().fill(Theme.hairline(scheme)).frame(height: 1).gridCellUnsizedAxes(.horizontal)
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    GridRow {
                        ForEach(0..<columns, id: \.self) { i in words(cell(row, i), size: 12) }
                    }
                }
            }
            .padding(.vertical, 2)
        }
        .scrollIndicators(.never)
    }

    private func styled(_ text: AttributedString) -> AttributedString {
        var out = text
        for run in out.runs where run.inlinePresentationIntent?.contains(.code) == true {
            out[run.range].font = .system(size: 12, design: .monospaced)
            out[run.range].backgroundColor = Color.primary.opacity(scheme == .dark ? 0.12 : 0.07)
        }
        return out
    }
}

/// A reply is on its way: three dots where it will be, and what's running
/// when it's a command, or that it's in line.
private struct MossThinking: View {
    var waiting = MossWaiting()
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            MossAvatar(size: 22)
            TimelineView(.animation(minimumInterval: 1 / 20, paused: reduceMotion)) { context in
                let t = context.date.timeIntervalSinceReferenceDate
                HStack(spacing: 8) {
                    HStack(spacing: 4) {
                        ForEach(0..<3) { i in
                            Circle()
                                .fill(Color.secondary)
                                .frame(width: 5, height: 5)
                                .opacity(reduceMotion ? 0.6 : 0.3 + 0.6 * max(0, sin((t * 2.2 - Double(i) * 0.35) * .pi)))
                        }
                    }
                    if let label {
                        label
                            .font(.system(size: 11.5))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
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

    /// "Running /usage…" (the command monospaced), "In line", or nothing.
    private var label: Text? {
        let name = waiting.command.map { Text($0).font(.system(size: 11, design: .monospaced)) }
        if waiting.queued {
            // Not running yet: say it's in line, and which command.
            let inLine = Text(L.t("In line", "排队中"))
            return name.map { inLine + Text(" · ") + $0 } ?? inLine
        }
        guard let name else { return nil }
        return L.lang == .zh ? Text("正在运行 ") + name + Text("…") : Text("Running ") + name + Text("…")
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
    /// Moss suggested it: a small sparkle on the corner.
    var pinned = false
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
                    .font(.system(size: compact ? 11.5 : 12, weight: .medium,
                                  design: MossCommand.looksLikeCommand(title) ? .monospaced : .default))
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
        .overlay(alignment: .topTrailing) {
            if pinned {
                Text(Image(systemName: "sparkle"))
                    .font(.system(size: 7.5, weight: .bold))
                    .foregroundStyle(Theme.brand)
                    .padding(2)
                    .background(Circle().fill(Theme.surface(scheme)))
                    .offset(x: 3, y: -4)
                    .allowsHitTesting(false)
            }
        }
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside && enabled } }
    }

    /// The chip's width as drawn (for fitting a row without guessing).
    static func width(title: String, icon: Bool) -> CGFloat {
        let font = NSFont.systemFont(ofSize: 12, weight: .medium)
        let text = ceil((title as NSString).size(withAttributes: [.font: font]).width)
        return text + (icon ? 14 + 5 : 0) + 20 + 2
    }
}

// MARK: - The quick bar

/// Moss's abilities and the account's commands in one row, the ones you
/// use most first. As many as fit; the rest are behind "⋯". The order is
/// worked out when the page opens and then holds still while you use it
/// (user 2026-10-19: re-ranking after a send made the bar jump); new uses
/// and Moss's picks show the next time the page opens.
private struct MossQuickRow: View {
    @Bindable var model: AppModel
    let enabled: Bool
    @State private var ranked: [MossQuickItem] = []
    @State private var pinned: Set<String> = []
    @State private var width: CGFloat = 0
    static let spacing: CGFloat = 6
    static let moreWidth: CGFloat = 34

    var body: some View {
        let shown = Self.fit(ranked, in: width)
        GeometryReader { geo in
            HStack(spacing: Self.spacing) {
                ForEach(ranked.prefix(shown)) { item in
                    MossChip(title: item.title(), icon: item.symbol, enabled: enabled, pinned: pinned.contains(item.id)) {
                        model.doQuick(item)
                    }
                    .tip(pinned.contains(item.id) ? L.t("Suggested by Moss · \(item.text())", "Moss 推荐 · \(item.text())")
                                                  : item.text())
                }
                if shown < ranked.count {
                    more
                }
            }
            .frame(maxHeight: .infinity)
            .onAppear { width = geo.size.width }
            .onChange(of: geo.size.width) { _, w in width = w }
        }
        .frame(height: 30)
        // Nothing on the page animates the bar along with it.
        .transaction { $0.animation = nil }
        .onAppear(perform: rank)
        .onChange(of: shown) { _, _ in publish() }
        .onChange(of: ranked) { _, _ in publish() }
        .onChange(of: model.mossDesk.status?.account) { _, _ in rank() }
    }

    private var more: some View {
        Button {
            model.mossQuickOpen.toggle()
        } label: {
            Image(systemName: "ellipsis")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(model.mossQuickOpen ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(.secondary))
                .frame(width: Self.moreWidth, height: 28)
                .background(Capsule().fill(model.mossQuickOpen ? Theme.brand.opacity(0.12) : Color.primary.opacity(0.05)))
                .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.45)
        .tip(L.t("Everything Moss can do", "全部快捷指令"))
        .accessibilityLabel(L.t("Everything Moss can do", "全部快捷指令"))
    }

    private func rank() {
        let context = model.quickContext
        ranked = MossQuick.ranked(model.quickItems, uses: model.quickUses, context: context, now: Date())
        pinned = Set(context.mossPicks)
        publish()
    }

    /// What "⋯" lists: what the row couldn't show.
    private func publish() {
        let more = MossQuick.overflow(ranked, shown: Self.fit(ranked, in: width))
        if more != model.quickOverflow { model.quickOverflow = more }
        if more.isEmpty { model.mossQuickOpen = false }
    }

    /// How many chips fit in `width` in order, leaving room for "⋯" when
    /// some don't. Chips never shrink or wrap.
    static func fit(_ items: [MossQuickItem], in width: CGFloat) -> Int {
        let widths = items.map { MossChip.width(title: $0.title(), icon: true) }
        let all = widths.reduce(0, +) + spacing * CGFloat(max(0, widths.count - 1)) + spacing + moreWidth
        if all <= width { return items.count }
        var used: CGFloat = moreWidth
        var count = 0
        for w in widths {
            if used + spacing + w > width { break }
            used += spacing + w
            count += 1
        }
        return count
    }
}

/// Everything the bar couldn't show, each once, in its order: Moss's
/// abilities, a hairline, then the commands.
private struct MossQuickMenu: View {
    @Bindable var model: AppModel
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let items = model.quickOverflow
        let actions = items.filter { $0.command == nil }
        let commands = items.filter { $0.command != nil }
        let picks = Set(model.quickContext.mossPicks)
        VStack(alignment: .leading, spacing: 2) {
            ForEach(actions) { item in row(item, pinned: picks.contains(item.id)) }
            if !actions.isEmpty && !commands.isEmpty {
                Rectangle().fill(Theme.hairline(scheme)).frame(height: 1).padding(.vertical, 3).padding(.horizontal, 6)
            }
            ForEach(commands) { item in row(item, pinned: picks.contains(item.id)) }
        }
        .padding(6)
        .frame(width: 268)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Theme.raised(scheme)))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Theme.hairline(scheme)))
        .shadow(color: .black.opacity(scheme == .dark ? 0.4 : 0.14), radius: 14, y: 4)
    }

    private func row(_ item: MossQuickItem, pinned: Bool) -> some View {
        MossQuickMenuRow(item: item, pinned: pinned) { model.doQuick(item) }
    }
}

private struct MossQuickMenuRow: View {
    let item: MossQuickItem
    let pinned: Bool
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Text(Image(systemName: item.symbol))
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 16)
                Text(item.title())
                    .font(.system(size: 12.5))
                    .lineLimit(1)
                if pinned {
                    Text(Image(systemName: "sparkle"))
                        .font(.system(size: 8, weight: .bold))
                        .foregroundStyle(Theme.brand)
                        .tip(L.t("Suggested by Moss", "Moss 推荐"))
                }
                Spacer(minLength: 6)
                if let command = item.command {
                    Text(command)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(.tertiary)
                }
            }
            .padding(.horizontal, 8)
            .frame(height: 28)
            .background(RoundedRectangle(cornerRadius: 7, style: .continuous).fill(hovering ? Theme.selection(scheme) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { hovering = $0 }
        .tip(item.text())
    }
}

// MARK: - Commands

/// A command Moss ran: the command on top, then its screen drawn as a card
/// (/usage, /status) with a toggle to the raw text, or the text as the
/// terminal had it (monospaced, spaces kept, scrolling sideways). A tall
/// screen is cut with a fade until you ask for all of it.
struct MossTerminalCard: View {
    let command: String
    let text: String
    /// The drawn version and the header's badge, when the screen was recognised.
    var graphic: (badge: String?, view: AnyView)?
    @State var raw = false
    /// Folds like any long reply (the message's id); nil: never folds.
    var fold: (id: String, model: AppModel)?
    @Environment(\.colorScheme) private var scheme

    /// One line ("unknown command", "That command printed nothing.") is a
    /// sentence: it wraps. A real screen keeps its columns and scrolls sideways.
    private var oneLine: Bool { !MossScreenText.withoutChrome(text).contains("\n") }

    @ViewBuilder private var screen: some View {
        let shown = MossScreenText.withoutChrome(text)
        let lines = Text(Self.screen(oneLine ? shown.trimmingCharacters(in: .whitespacesAndNewlines) : shown))
            .font(.system(size: 11, design: .monospaced))
            .lineSpacing(1.5)
            .textSelection(.enabled)
        if oneLine {
            lines
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 11)
                .padding(.vertical, 9)
        } else {
            ScrollView(.horizontal) {
                lines
                    .fixedSize()
                    .padding(.horizontal, 11)
                    .padding(.vertical, 9)
            }
            .scrollIndicators(.never)
        }
    }

    /// The screen's text; block characters (the usage bars) in the brand colour,
    /// as the terminal draws them in colour.
    static func screen(_ raw: String) -> AttributedString {
        var out = AttributedString(raw.trimmingCharacters(in: .newlines))
        var i = out.startIndex
        while i < out.endIndex {
            let next = out.index(afterCharacter: i)
            if let scalar = out.characters[i].unicodeScalars.first, (0x2580...0x259F).contains(scalar.value) {
                out[i..<next].foregroundColor = Theme.brand
            }
            i = next
        }
        return out
    }

    private var header: some View {
        HStack(spacing: 6) {
            HStack(spacing: 5) {
                Text(Image(systemName: "terminal"))
                    .font(.system(size: 10, weight: .semibold))
                Text(command)
                    .font(.system(size: 11, weight: .semibold, design: .monospaced))
            }
            .foregroundStyle(Theme.brand)
            .padding(.horizontal, 7)
            .frame(height: 20)
            .background(Capsule().fill(Theme.brand.opacity(scheme == .dark ? 0.18 : 0.10)))
            if let badge = graphic?.badge {
                Text(badge)
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 7)
                    .frame(height: 20)
                    .overlay(Capsule().strokeBorder(Theme.hairline(scheme)))
            }
            Spacer(minLength: 6)
            if graphic != nil {
                Button {
                    withAnimation(.easeOut(duration: 0.18)) { raw.toggle() }
                } label: {
                    HStack(spacing: 4) {
                        Text(Image(systemName: raw ? "chart.bar.xaxis" : "text.alignleft"))
                            .font(.system(size: 9.5, weight: .semibold))
                        Text(raw ? L.t("Visual", "图示") : L.t("Raw", "原文"))
                    }
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 7)
                    .frame(height: 20)
                    .background(Capsule().fill(Color.primary.opacity(scheme == .dark ? 0.07 : 0.045)))
                    .contentShape(Capsule())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
                .tip(raw ? L.t("Show it drawn", "看图示") : L.t("Show the terminal text", "看终端原文"))
                .accessibilityLabel(raw ? L.t("Show it drawn", "看图示") : L.t("Show the terminal text", "看终端原文"))
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 9)
        .padding(.bottom, 7)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Rectangle().fill(Theme.hairline(scheme)).frame(height: 1)
            if let fold {
                MossFoldable(id: fold.id, model: fold.model) { content }
            } else {
                content
            }
        }
        .background(ChatBubble(mine: false).fill(Theme.raised(scheme)))
        .clipShape(ChatBubble(mine: false))
        .overlay(ChatBubble(mine: false).stroke(Theme.hairline(scheme), lineWidth: 1))
        .shadow(color: .black.opacity(scheme == .dark ? 0.0 : 0.04), radius: 2, y: 1)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(L.t("\(command) result", "\(command) 的结果"))
    }

    @ViewBuilder private var content: some View {
        if let graphic, !raw {
            graphic.view
                .transition(.opacity)
        } else {
            screen
        }
    }
}

/// A long reply, folded (user 2026-10-19): cut at `MossFold.height` with a
/// fade and "Show more". The one fold for words and command cards alike;
/// what you unfold stays open while the page is.
struct MossFoldable<Content: View>: View {
    let id: String
    @Bindable var model: AppModel
    @ViewBuilder let content: () -> Content
    @State private var height: CGFloat = 0
    @Environment(\.colorScheme) private var scheme

    private var long: Bool { MossFold.folds(Double(height)) }
    private var folded: Bool { long && !model.mossExpanded.contains(id) }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content()
                .fixedSize(horizontal: false, vertical: true)
                .background(GeometryReader { geo in
                    Color.clear.preference(key: FoldHeightKey.self, value: geo.size.height)
                })
                .frame(height: folded ? CGFloat(MossFold.height) : nil, alignment: .top)
                .clipped()
                .overlay(alignment: .bottom) {
                    if folded {
                        LinearGradient(colors: [Theme.raised(scheme).opacity(0), Theme.raised(scheme)],
                                       startPoint: .top, endPoint: .bottom)
                            .frame(height: 48)
                            .allowsHitTesting(false)
                    }
                }
                .onPreferenceChange(FoldHeightKey.self) { height = $0 }
            if long {
                Rectangle().fill(Theme.hairline(scheme)).frame(height: 1)
                Button {
                    withAnimation(.easeOut(duration: 0.2)) {
                        if folded { model.mossExpanded.insert(id) } else { model.mossExpanded.remove(id) }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text(folded ? L.t("Show more", "展开") : L.t("Show less", "收起"))
                        Image(systemName: folded ? "chevron.down" : "chevron.up")
                            .font(.system(size: 9, weight: .semibold))
                    }
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .frame(height: 26)
                    .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
            }
        }
    }
}

private struct FoldHeightKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

/// The commands Moss can run, above the field while you type a slash.
private struct MossCommandList: View {
    let commands: [MossCommand]
    let lit: Int
    let pick: (MossCommand) -> Void
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(spacing: 1) {
            ForEach(Array(commands.enumerated()), id: \.element.id) { i, command in
                Button { pick(command) } label: {
                    HStack(spacing: 10) {
                        Text(command.name)
                            .font(.system(size: 12, weight: .medium, design: .monospaced))
                            .foregroundStyle(i == lit ? Theme.brand : .primary)
                            .frame(width: 82, alignment: .leading)
                        Text(command.detail())
                            .font(.system(size: 12))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                        Spacer(minLength: 4)
                        if i == lit {
                            Text("Tab")
                                .font(.system(size: 10, weight: .medium))
                                .foregroundStyle(.tertiary)
                        }
                    }
                    .padding(.horizontal, 9)
                    .frame(height: 26)
                    .background(RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(i == lit ? Theme.brand.opacity(scheme == .dark ? 0.16 : 0.08) : .clear))
                    .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
            }
        }
        .padding(4)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Theme.raised(scheme)))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.hairline(scheme)))
        .shadow(color: .black.opacity(scheme == .dark ? 0.3 : 0.06), radius: 6, y: 2)
    }
}

// MARK: - The bottom bar

/// Moss's abilities as buttons, plus a keyboard to type instead; or the
/// text field, with a way back to the buttons.
private struct MossInputBar: View {
    @Bindable var model: AppModel
    let keys: (KeyPress) -> KeyPress.Result
    @FocusState private var focused: Bool
    /// The lit command in the list, moved with ↑↓.
    @State private var lit = 0
    /// Moss's guess in the field: quiet until you touch it.
    @State private var guess = SuggestionField()
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
            MossQuickRow(model: model, enabled: blocked == nil)
            MossIconButton(symbol: "keyboard",
                           tip: suggestion.map { L.t("Moss's guess: \($0)", "Moss 猜你想说：\($0)") } ?? L.t("Type a message", "打字")) {
                model.mossTyping = true
            }
                .disabled(blocked != nil)
                .opacity(blocked == nil ? 1 : 0.45)
        }
    }

    private var text: String {
        get { model.mossDraft }
        nonmutating set { model.mossDraft = newValue }
    }

    /// Claude Code's guess at what you'd say to Moss, when it has one.
    private var suggestion: String? { Suggestion.prefill(text: "", touched: false, suggestion: model.mossSuggestion) }

    /// Put Moss's guess in the field, quietly selected, unless you've typed.
    private func prefill() {
        guard let fill = guess.offer(model.mossSuggestion, text: text) else { return }
        text = fill
        FieldSelection.selectAllQuietly()
    }

    private func engage() {
        guard guess.untouched else { return }
        guess.engaged()
        FieldSelection.showHighlight()
    }

    /// Commands matching what's typed: a slash and no space yet.
    private var commands: [MossCommand] {
        MossCommand.suggestions(for: text, account: desk.status?.account ?? .unknown)
    }

    private var typing: some View {
        VStack(alignment: .leading, spacing: 6) {
            if !commands.isEmpty {
                MossCommandList(commands: commands, lit: min(lit, commands.count - 1)) { fill($0) }
                    .transition(.opacity.combined(with: .move(edge: .bottom)))
            }
            field
        }
        .animation(.easeOut(duration: 0.12), value: commands)
        .onChange(of: text) { _, _ in lit = 0 }
    }

    private var field: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField(L.t("Message Moss…", "和 Moss 说…"), text: $model.mossDraft, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 13, design: MossCommand.looksLikeCommand(text) ? .monospaced : .default))
                // Moss's untouched guess is as quiet as a placeholder.
                .foregroundStyle(guess.untouched ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.primary))
                .simultaneousGesture(TapGesture().onEnded { engage() })
                .onKeyPress(keys: [.leftArrow, .rightArrow]) { _ in engage(); return .ignored }
                .lineLimit(1...5)
                .focused($focused)
                .onKeyPress(.return, phases: .down) { press in
                    if press.modifiers.contains(.shift) {
                        // A line break where the cursor is, the way AppKit's own fields do it.
                        NSApp.sendAction(#selector(NSResponder.insertNewlineIgnoringFieldEditor(_:)), to: nil, from: nil)
                    } else if let command = litCommand {
                        // A command picked from the list: Return sends it.
                        model.sayToMoss(command.name)
                        text = ""
                    } else {
                        send()
                    }
                    return .handled
                }
                .onKeyPress(.downArrow) { move(1) }
                .onKeyPress(.upArrow) { move(-1) }
                .onKeyPress(.tab) {
                    guard let command = litCommand else { return .ignored }
                    fill(command)
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
            .accessibilityLabel(L.t("Send", "发送"))
            .padding(.bottom, 1)
            MossIconButton(symbol: "square.grid.2x2", tip: L.t("Back to actions", "回到常用操作")) { model.mossTyping = false }
                .padding(.bottom, 1)
        }
        .onAppear {
            focused = true
            if !text.isEmpty { guess.changed(to: text) }
            prefill()
            model.fetchMossSuggestion()
        }
        .onChange(of: model.mossSuggestion) { _, _ in prefill() }
        .onChange(of: text) { _, new in
            let was = guess.untouched
            guess.changed(to: new)
            if was && !guess.untouched { FieldSelection.showHighlight() }
        }
    }

    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private var litCommand: MossCommand? {
        let list = commands
        return list.isEmpty ? nil : list[min(lit, list.count - 1)]
    }

    private func move(_ step: Int) -> KeyPress.Result {
        let count = commands.count
        guard count > 0 else { return .ignored }
        lit = (min(lit, count - 1) + step + count) % count
        return .handled
    }

    /// Tab or a click: the command goes into the field, ready to send.
    private func fill(_ command: MossCommand) {
        text = command.name
        focused = true
    }

    private func send() {
        guard !empty else { return }
        model.sayToMoss(text)
        text = ""
        // A fresh field: Moss's next guess may fill it.
        guess.reset()
    }
}

/// A borderless square icon button.
private struct MossIconButton: View {
    let symbol: String
    let tip: String
    /// Something waiting behind it (Moss's guess at your reply).
    var dot = false
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
                .overlay(alignment: .topTrailing) {
                    if dot {
                        Circle().fill(Theme.brand)
                            .frame(width: 6, height: 6)
                            .padding(1.5)
                            .background(Circle().fill(Theme.surface(scheme)))
                            .offset(x: 1, y: 2)
                            .transition(.scale.combined(with: .opacity))
                    }
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hovering = inside } }
        .tip(tip)
        .accessibilityLabel(tip)
    }
}

/// Moss's guess at your reply, floating faintly over the quick bar: a click
/// opens typing with it filled in; × puts it away until Moss says something new.
extension AppModel {
    private var newestMossReply: String { mossDesk.chat.last { $0.from == .moss }?.id ?? "none" }

    /// Moss's guess at your reply, shown above the bar: unless you're typing,
    /// closed it for this reply, or Moss can't take a message.
    var mossShownGuess: String? {
        guard !mossTyping, mossDesk.status?.inputBlocked() == nil,
              let s = Suggestion.prefill(text: "", touched: false, suggestion: mossSuggestion),
              mossGuessHiddenFor != newestMossReply else { return nil }
        return s
    }

    func hideMossGuess() {
        mossGuessHiddenFor = newestMossReply
    }
}

/// The guess under the conversation, on the page's background.
private struct MossGuessInset: View {
    @Bindable var model: AppModel

    var body: some View {
        Group {
            if let guess = model.mossShownGuess {
                MossGuessBadge(text: guess) {
                    model.mossTyping = true
                } close: {
                    model.hideMossGuess()
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.18), value: model.mossShownGuess)
    }
}

/// "Jump to latest": a small round button over the conversation's end, with
/// how many of Moss's messages came while you were scrolled up.
private struct JumpToLatest: View {
    let count: Int
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if count > 0 {
                    Text(L.t("\(count) new", "\(count) 条新消息"))
                        .font(.system(size: 11.5, weight: .semibold))
                        .foregroundStyle(Theme.brand)
                        .padding(.leading, 4)
                }
                Image(systemName: "arrow.down")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(hovering ? .primary : .secondary)
            }
            .padding(.horizontal, count > 0 ? 10 : 0)
            .frame(minWidth: 32, minHeight: 32)
            .background(Capsule().fill(Theme.raised(scheme)))
            .overlay(Capsule().strokeBorder(Theme.hairline(scheme)))
            .overlay(alignment: .topTrailing) {
                if count > 0 {
                    Circle().fill(Theme.brand).frame(width: 7, height: 7).offset(x: -2, y: 2)
                }
            }
            .shadow(color: .black.opacity(scheme == .dark ? 0.4 : 0.12), radius: 8, y: 3)
            .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .handCursor()
        .onHover { hovering = $0 }
        .tip(L.t("Jump to latest", "回到最新"))
        .accessibilityLabel(L.t("Jump to latest", "回到最新"))
    }
}

private struct MossGuessBadge: View {
    let text: String
    let open: () -> Void
    let close: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        HStack(spacing: 6) {
            Button(action: open) {
                HStack(spacing: 5) {
                    Text("↳").foregroundStyle(.tertiary)
                    Text(text)
                        .foregroundStyle(hovering ? .secondary : .tertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                .font(.system(size: 11.5))
                .contentShape(Rectangle())
            }
            .buttonStyle(PressStyle())
            .focusable(false)
            if hovering {
                Button(action: close) {
                    Image(systemName: "xmark")
                        .font(.system(size: 8.5, weight: .bold))
                        .foregroundStyle(.tertiary)
                        .frame(width: 14, height: 14)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
                .tip(L.t("Hide until Moss's next reply", "先收起，等 Moss 下次回复"))
                .accessibilityLabel(L.t("Hide the guess", "收起猜测"))
            }
        }
        .padding(.horizontal, 9)
        .frame(height: 24)
        .background(Capsule().fill(Color.primary.opacity(scheme == .dark ? 0.05 : 0.035)))
        .frame(maxWidth: 360, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
        .tip(L.t("Moss's guess at your reply. Click to use it.", "Moss 猜你想说的话，点一下就用它。"))
    }
}
