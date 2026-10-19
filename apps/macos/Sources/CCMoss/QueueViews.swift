import MossKit
import SwiftUI

// The panel's side rail and its "To Do" page (user 2026-10-18): everything
// waiting on you as one list, worked through one item at a time, while the
// sessions page stays free to use.

/// The rail down the panel's left edge: one button per page, Settings at the
/// foot. Pages to come (Doctor) take a place here too.
struct NavRail: View {
    @Bindable var model: AppModel
    /// The gear: cc-moss's own menu (Settings, Refresh, the version, Quit).
    let appMenu: () -> Void
    var appMenuOpen = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(spacing: 4) {
            RailButton(icon: "rectangle.stack", title: L.t("Sessions", "会话"), shortcut: "⌘1", key: "1",
                       selected: model.page == .sessions) { model.page = .sessions }
            RailButton(icon: "tray.full", title: L.t("To Do", "待处理"), shortcut: "⌘2", key: "2",
                       selected: model.page == .queue, badge: model.queueCount) { model.showQueue() }
            RailButton(icon: "bubble.left.and.text.bubble.right", title: "Moss", shortcut: "⌘3", key: "3",
                       selected: model.page == .moss, dot: mossDot) { model.showMoss() }
            Spacer(minLength: 0)
            // A bare gear, as menu bar apps have; an update puts a dot on it.
            RailButton(icon: Icons.settings, title: L.t("Settings", "设置"), shortcut: "⌘,",
                       selected: appMenuOpen, dot: model.update != nil ? .unread : nil, showsTitle: false,
                       action: appMenu)
        }
        .padding(.top, 8)
        .padding(.bottom, 10)
        .frame(width: Theme.railWidth)
        .frame(maxHeight: .infinity)
        .background(Color.primary.opacity(scheme == .dark ? 0.035 : 0.03))
        .overlay(alignment: .trailing) {
            Rectangle().fill(Theme.hairline(scheme)).frame(width: 1)
        }
    }

    /// An answer you haven't read beats Moss thinking; on its page, neither shows.
    private var mossDot: RailDot? {
        guard model.page != .moss else { return nil }
        if model.mossDesk.unread { return .unread }
        // Moss isn't in the session list any more: a broken or switched-off
        // Moss still shows, quietly, here.
        switch model.mossDesk.activity {
        case .thinking: return .busy
        case .offline: return .trouble
        case .off: return .off
        default: return nil
        }
    }
}

/// A small dot on a rail icon: something new there, or work going on.
enum RailDot: Equatable {
    case unread
    /// Breathes.
    case busy
    /// Something's wrong there (Moss can't start): amber.
    case trouble
    /// Switched off: grey.
    case off

    var color: Color {
        switch self {
        case .unread, .busy: Theme.brand
        case .trouble: Theme.amber
        case .off: Color.secondary
        }
    }
}

/// One page in the rail: its symbol over a short name; the page you're on
/// is lit in the brand colour, a count rides on its corner.
struct RailButton: View {
    let icon: String
    let title: String
    var shortcut: String?
    /// ⌘ and this key go to the page wherever the keyboard is, even when no
    /// field has it (the To Do and Moss pages).
    var key: KeyEquivalent?
    let selected: Bool
    var badge = 0
    var dot: RailDot?
    /// The name under the icon; the gear goes without (it's understood).
    var showsTitle = true
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            VStack(spacing: 3) {
                Image(systemName: icon)
                    .font(.system(size: 15, weight: selected ? .semibold : .regular))
                    .frame(width: 36, height: 28)
                    .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(tileFill))
                    .overlay(alignment: .topTrailing) {
                        if badge > 0 {
                            Text(badge > 99 ? "99+" : "\(badge)")
                                .font(.system(size: 9.5, weight: .bold))
                                .monospacedDigit()
                                .foregroundStyle(.white)
                                .padding(.horizontal, 4)
                                .frame(minWidth: 16, minHeight: 16)
                                .background(Capsule().fill(Theme.brand))
                                .overlay(Capsule().strokeBorder(Theme.surface(scheme), lineWidth: 1.5))
                                .offset(x: 6, y: -5)
                                .transition(.scale.combined(with: .opacity))
                        } else if let dot {
                            MossDot(color: dot.color, breathing: dot == .busy, size: 8)
                                .padding(2)
                                .background(Circle().fill(Theme.surface(scheme)))
                                .offset(x: 3, y: -3)
                                .transition(.scale.combined(with: .opacity))
                        }
                    }
                if showsTitle {
                    Text(title)
                        .font(.system(size: 9.5, weight: selected ? .semibold : .medium))
                        .lineLimit(1)
                        .fixedSize()
                }
            }
            .foregroundStyle(selected ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(hovering ? .primary : .secondary))
            .frame(width: Theme.railWidth, height: showsTitle ? 48 : 36)
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .commandKey(key)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
        .tip(shortcut.map { "\(title)  \($0)" } ?? title)
        .accessibilityLabel(badge > 0 ? "\(title), \(badge)" : dot == .unread ? L.t("\(title), new reply", "\(title)，有新回复") : title)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .animation(.spring(response: 0.3, dampingFraction: 0.7), value: badge)
        .animation(.easeOut(duration: 0.2), value: dot)
    }

    private var tileFill: Color {
        if selected { return Theme.brand.opacity(scheme == .dark ? 0.20 : 0.12) }
        return hovering ? Theme.selection(scheme) : .clear
    }
}

// MARK: - The "To Do" page

/// Questions and replies waiting on you, oldest first. One item is open at
/// a time; answer it and the next one opens. Nothing here blocks the
/// sessions page: switch over, come back, the queue is where you left it.
struct QueuePage: View {
    @Bindable var model: AppModel
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let items = model.queueItems
        VStack(spacing: 0) {
            header(items)
            Rectangle().fill(Theme.hairline(scheme)).frame(height: 1)
            if items.isEmpty {
                empty
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 8) {
                            ForEach(items, id: \.self) { item in
                                QueueRow(model: model, item: item, open: isOpen(item, in: items)) {
                                    toggle(item, in: items)
                                }
                                .id(item)
                            }
                        }
                        .padding(10)
                    }
                    .onChange(of: model.askQueue.shown(in: items)) { _, item in
                        guard let item, model.askQueue.expanded else { return }
                        withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(item, anchor: .top) }
                    }
                }
            }
        }
        .animation(.easeOut(duration: 0.18), value: model.askQueue)
        .animation(.easeOut(duration: 0.18), value: items)
    }

    private func isOpen(_ item: AskQueue.Item, in items: [AskQueue.Item]) -> Bool {
        model.askQueue.expanded && model.askQueue.shown(in: items) == item
    }

    private func toggle(_ item: AskQueue.Item, in items: [AskQueue.Item]) {
        if isOpen(item, in: items) {
            model.askQueue.expanded = false
        } else {
            model.askQueue.show(item, in: items)
        }
    }

    private func header(_ items: [AskQueue.Item]) -> some View {
        HStack(spacing: 8) {
            Text(L.t("To Do", "待处理"))
                .font(.system(size: 15, weight: .semibold))
            if model.queueCount > 0 {
                Text("\(model.queueCount)")
                    .font(.system(size: 11, weight: .semibold))
                    .monospacedDigit()
                    .foregroundStyle(Theme.brand)
                    .padding(.horizontal, 6)
                    .frame(height: 18)
                    .background(Capsule().fill(Theme.brand.opacity(scheme == .dark ? 0.18 : 0.10)))
            }
            Spacer(minLength: 8)
            if !items.isEmpty {
                let open = model.askQueue.expanded
                QuietButton(title: open ? L.t("Collapse", "收起") : L.t("Open Next", "处理下一件"),
                            icon: open ? "chevron.up" : "chevron.down") {
                    model.askQueue.expanded = !open
                }
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 50)
    }

    private var empty: some View {
        VStack(spacing: 8) {
            Image(systemName: "checkmark.circle")
                .font(.system(size: 26, weight: .light))
                .foregroundStyle(.tertiary)
            Text(L.t("Nothing waiting on you", "没有在等你的事"))
                .font(.system(size: 13, weight: .medium))
            Text(L.t("When a session asks you something or waits for your reply, it lines up here.",
                     "会话问你问题、或者等你回话时，会在这里排队。"))
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 260)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(.bottom, 30)
    }
}

/// One item in the queue. Closed, it's one line: who, and what they want.
/// Open, it's the whole question and its answers, or a reply box.
private struct QueueRow: View {
    @Bindable var model: AppModel
    let item: AskQueue.Item
    let open: Bool
    let toggle: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button(action: toggle) { heading }
                .buttonStyle(PressStyle())
                .focusable(false)
                .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
                .handCursor()
            if open {
                detail
                    .padding(.leading, 40)
                    .padding(.trailing, 12)
                    .padding(.bottom, 12)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(fill))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(border))
        .accessibilityElement(children: .contain)
    }

    private var fill: Color {
        if open { return Theme.raised(scheme) }
        return hovering ? Theme.selection(scheme) : Color.primary.opacity(scheme == .dark ? 0.03 : 0.02)
    }

    private var border: Color {
        open ? Theme.brand.opacity(0.35) : Theme.hairline(scheme)
    }

    private var heading: some View {
        HStack(alignment: .center, spacing: 10) {
            tile
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if !open, let summary {
                    Text(summary)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
            Spacer(minLength: 6)
            if let step {
                Text(step)
                    .font(.system(size: 11))
                    .monospacedDigit()
                    .foregroundStyle(.tertiary)
            }
            Image(systemName: "chevron.down")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(.tertiary)
                .rotationEffect(.degrees(open ? 180 : 0))
        }
        .padding(.horizontal, 12)
        .frame(minHeight: 52)
        .contentShape(Rectangle())
    }

    @ViewBuilder private var tile: some View {
        switch item {
        case let .ask(id):
            AskTile(kind: model.askDesk.card(id)?.ask.kind ?? .question)
        case .reply:
            ZStack {
                RoundedRectangle(cornerRadius: 6).fill(Theme.brand.opacity(scheme == .dark ? 0.22 : 0.14))
                Image(systemName: Icons.message)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Theme.brand)
            }
            .frame(width: 22, height: 22)
            .accessibilityHidden(true)
        }
    }

    private var card: AskCard? {
        if case let .ask(id) = item { return model.askDesk.card(id) }
        return nil
    }

    private var session: Session? {
        if case let .reply(id) = item { return model.sessions.first { $0.id == id } }
        return nil
    }

    private var title: String {
        if let card {
            let name = model.askTitle(card.ask)
            return card.ask.kind == .permission ? L.t("\(name) wants your OK", "\(name) 想征得你同意")
                                                : L.t("\(name) asks", "\(name) 问你")
        }
        if let session { return L.t("\(session.title) is waiting for you", "\(session.title) 在等你回话") }
        return ""
    }

    /// The closed row's second line: what it wants, in its own words.
    private var summary: String? {
        if let card {
            if case let .finished(outcome) = card.phase { return AskQueueText.outcome(outcome) }
            if card.ask.kind == .permission {
                let tool = card.ask.tool
                return [tool?.name, tool?.preview].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
            }
            return card.question?.text
        }
        if session != nil { return L.t("It's waiting for your reply.", "它在等你回话。") }
        return nil
    }

    private var step: String? {
        guard let card, card.several, !card.reviewing, card.phase == .choosing else { return nil }
        return "\(card.step + 1)/\(card.count)"
    }

    @ViewBuilder private var detail: some View {
        if let card {
            AskCardBody(card: card, sessionTitle: model.askTitle(card.ask)) { body in
                model.act(on: card.id, body)
            }
        } else if let session {
            ReplyBox(model: model, session: session)
        }
    }
}

enum AskQueueText {
    static func outcome(_ outcome: AskOutcome) -> String {
        switch outcome {
        case .answered: L.t("Answered", "答好了")
        case .late: L.t("Too late, it moved on", "晚了一步，它已经往下走了")
        case .notWaiting: L.t("Saved for when it asks again", "答案先记下了")
        case let .failed(message): message
        }
    }
}

// MARK: - The nudge on the sessions page

/// Something new is waiting while you're on the sessions page: one quiet
/// line above the action bar. It doesn't cover a row you might be using and
/// doesn't take the keyboard; "Handle" goes to the To Do page on it.
struct QueueNudge: View {
    @Bindable var model: AppModel
    @Environment(\.colorScheme) private var scheme

    static func shows(_ model: AppModel) -> Bool {
        model.phase == .ready && model.page == .sessions && model.queueCount > 0
    }

    var body: some View {
        let items = model.queueItems.filter { item in
            if case let .ask(id) = item { return model.askDesk.open.contains { $0.id == id } }
            return true
        }
        if let first = items.first {
            HStack(spacing: 10) {
                Image(systemName: "tray.full")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 22, height: 22)
                    .background(RoundedRectangle(cornerRadius: 6).fill(Theme.brand.opacity(scheme == .dark ? 0.22 : 0.14)))
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 6) {
                        Text(who(first))
                            .font(.system(size: 12, weight: .semibold))
                            .lineLimit(1)
                            .truncationMode(.middle)
                        if items.count > 1 {
                            Text(L.t("+\(items.count - 1) more", "还有 \(items.count - 1) 件"))
                                .font(.system(size: 11))
                                .foregroundStyle(.tertiary)
                                .fixedSize()
                        }
                    }
                    if let what = what(first) {
                        Text(what)
                            .font(.system(size: 11.5))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                }
                Spacer(minLength: 6)
                SoftButton(title: L.t("Handle", "去处理"), icon: "arrow.right", prominent: true) {
                    model.page = .queue
                    model.askQueue.show(first, in: model.queueItems)
                }
                .focusable(false)
            }
            .padding(.leading, 10)
            .padding(.trailing, 8)
            .frame(height: 46)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Theme.raised(scheme)))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Theme.brand.opacity(0.30)))
            .shadow(color: .black.opacity(scheme == .dark ? 0.35 : 0.12), radius: 10, y: 3)
            .accessibilityElement(children: .combine)
        }
    }

    private func who(_ item: AskQueue.Item) -> String {
        switch item {
        case let .ask(id):
            guard let card = model.askDesk.card(id) else { return "" }
            let name = model.askTitle(card.ask)
            return card.ask.kind == .permission ? L.t("\(name) wants your OK", "\(name) 想征得你同意")
                                                : L.t("\(name) asks", "\(name) 问你")
        case let .reply(id):
            let name = model.sessions.first { $0.id == id }?.title ?? ""
            return L.t("\(name) is waiting for you", "\(name) 在等你回话")
        }
    }

    /// What it wants, in its own words.
    private func what(_ item: AskQueue.Item) -> String? {
        guard case let .ask(id) = item, let card = model.askDesk.card(id) else { return nil }
        if card.ask.kind == .permission { return card.ask.tool.map { "\($0.name) · \($0.preview)" } }
        return card.question?.text
    }
}

private extension View {
    @ViewBuilder func commandKey(_ key: KeyEquivalent?) -> some View {
        if let key { keyboardShortcut(key, modifiers: .command) } else { self }
    }
}
