import AppKit
import MossKit
import SwiftUI

/// The sessions Moss points at, as small cards under its words (T2.3
/// AMEND-4): open or bring back, rename in place, pin, like a Sessions row.
struct MossSessionCards: View {
    @Bindable var model: AppModel
    let messageID: String
    let ids: [String]

    private var cards: [MossSessionCard] {
        MossSessionCard.cards(ids, among: model.sessions + (model.closedPool ?? []))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(cards) { card in
                if let s = card.session {
                    MossSessionCardView(model: model, card: card, session: s, key: "\(messageID)/\(card.id)")
                } else {
                    GoneCard()
                }
            }
        }
        .frame(maxWidth: 420, alignment: .leading)
        .onAppear {
            // Moss may point at one that has ended: those come from `ls --all`.
            let known = Set(model.sessions.map(\.id))
            if model.closedPool == nil, ids.contains(where: { !known.contains($0) }) { model.loadClosed() }
        }
    }
}

/// A session that isn't there any more.
private struct GoneCard: View {
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        HStack(spacing: 8) {
            ZStack {
                RoundedRectangle(cornerRadius: 5).fill(Color.primary.opacity(0.06))
                Image(systemName: "questionmark")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
            .frame(width: 20, height: 20)
            Text(L.t("This session is gone", "这个会话已经不在了"))
                .font(.system(size: 12))
                .foregroundStyle(.tertiary)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .frame(height: 34)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(scheme == .dark ? 0.03 : 0.025)))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Theme.hairline(scheme), style: StrokeStyle(lineWidth: 1, dash: [3, 3])))
    }
}

private struct MossSessionCardView: View {
    @Bindable var model: AppModel
    let card: MossSessionCard
    let session: Session
    /// Which card's rename field is open: message and session together, so
    /// the same session in two replies doesn't open twice.
    let key: String
    @State private var name = ""
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    private var renaming: Bool { model.mossCardRenaming == key }
    private var pinned: Bool { model.pins.contains(session.id) }
    private var restoring: Bool { card.main == .bringBack && model.restoreFlow.isRunning(session.id) }
    private var check: Message.RenameCheck { Message.checkRename(name, of: session, among: model.sessions) }
    // The move into tmux shares the list's state: started here or there, both show it.
    private var asking: Bool { model.adoptAsk?.id == session.id }
    private var moving: Bool { model.adopting.contains(session.id) }
    private var adoptProblem: String? { model.adoptProblems[session.id] }
    private var note: String? { model.rowNote.flatMap { $0.id == session.id && !$0.adopt ? $0.text : nil } }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                stateIcon
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 5) {
                        Text(session.title)
                            .font(.system(size: 12.5, weight: .medium))
                            .lineLimit(1)
                        if !session.inTmux {
                            Image(systemName: Icons.outsideTmux)
                                .font(.system(size: 9.5, weight: .semibold))
                                .foregroundStyle(.tertiary)
                                .tip(L.t("Not in tmux, so it can't be opened from here.", "不在 tmux 里，没法从这里打开。"))
                                .accessibilityLabel(L.t("not in tmux", "不在 tmux 里"))
                        }
                    }
                    HStack(spacing: 4) {
                        Text(PanelView.folderName(session))
                        Text("·")
                        Text(L.ago(session.lastActiveAt))
                    }
                    .font(.system(size: 11))
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                }
                Spacer(minLength: 6)
                actions
            }
            if renaming { renameLine }
            if asking {
                AdoptConfirmView(model: model, session: session, inset: 0, trailing: 0)
            } else if moving {
                cardLine(L.t("Moving into tmux…", "正在移进 tmux…"), spinner: true)
            } else if let adoptProblem {
                cardLine(adoptProblem, icon: Icons.cannotRestore, tint: Theme.amber)
            } else if let note {
                cardLine(note, icon: "info.circle")
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 8).fill(Theme.raised(scheme)))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Theme.hairline(scheme), lineWidth: 1))
    }

    /// A short line under the card: moving, why it couldn't, why it waits.
    private func cardLine(_ text: String, icon: String? = nil, tint: Color? = nil, spinner: Bool = false) -> some View {
        HStack(spacing: 5) {
            if spinner {
                ProgressView().controlSize(.mini)
            } else if let icon {
                Image(systemName: icon)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(tint.map(AnyShapeStyle.init) ?? AnyShapeStyle(.tertiary))
            }
            Text(text).lineLimit(2).fixedSize(horizontal: false, vertical: true)
        }
        .font(.system(size: 11.5))
        .foregroundStyle(.secondary)
        .padding(.leading, 28)
        .tip(text)
    }

    @ViewBuilder private var actions: some View {
        HStack(spacing: 2) {
            if card.canPin && !moving {
                PinButton(pinned: pinned) { model.togglePin(session) }
            }
            if card.canRename && !moving {
                CardIconButton(icon: Icons.rename, help: L.t("Rename", "改名"), lit: renaming) {
                    if renaming {
                        model.mossCardRenaming = nil
                    } else {
                        name = session.name ?? ""
                        model.mossCardRenaming = key
                    }
                }
            }
            switch card.main {
            case .open?:
                CardTextButton(title: L.t("Open", "打开"), icon: Icons.open) {
                    let panel = NSApplication.shared.keyWindow
                    model.open(session) { opened in if opened { panel?.close() } }
                }
            case .bringBack?:
                if restoring {
                    ProgressView().controlSize(.small).scaleEffect(0.6).frame(width: 24, height: 22)
                } else {
                    CardTextButton(title: L.t("Bring back", "找回"), icon: Icons.restore) { model.restore(session) }
                }
            case .moveIntoTmux?:
                if !moving {
                    let wait = AppModel.adoptWait(session)
                    CardTextButton(title: AppModel.adoptTitle(session, named: model.adoptAsk?.id == session.id ? model.adoptAsk?.named : nil),
                                   icon: Icons.adopt, dimmed: wait != nil, lit: asking) {
                        if asking { model.cancelAdopt() } else { model.askToAdopt(session) }
                    }
                    .tip(wait ?? L.t("Ends its process in its current terminal and carries on in tmux; the conversation is kept.",
                                     "结束它在原终端里的进程，然后在 tmux 里接着跑，对话不会丢。"))
                }
            case nil:
                EmptyView()
            }
        }
    }

    private var renameLine: some View {
        HStack(spacing: 6) {
            TextField(L.t("New name", "新名字"), text: $name)
                .textFieldStyle(.plain)
                .font(.system(size: 12))
                .focused($focused)
                .onSubmit(save)
                .onExitCommand { model.mossCardRenaming = nil }
                .padding(.horizontal, 7)
                .frame(height: 24)
                .background(RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(scheme == .dark ? 0.08 : 0.05)))
                .overlay(RoundedRectangle(cornerRadius: 6)
                    .strokeBorder(problem == nil ? Theme.brand.opacity(0.5) : Theme.amber.opacity(0.7), lineWidth: 1))
            if let problem {
                Text(problem)
                    .font(.system(size: 11))
                    .foregroundStyle(Theme.amber)
                    .lineLimit(1)
            } else {
                Text(L.t("Return saves", "回车保存"))
                    .font(.system(size: 11))
                    .foregroundStyle(.tertiary)
            }
        }
        .onAppear {
            if name.isEmpty { name = session.name ?? "" }
            focused = true
        }
    }

    /// What's wrong with the name typed, said briefly; nil when it can be saved
    /// (or nothing's been typed yet).
    private var problem: String? {
        switch check {
        case .ok, .unchanged, .empty: return nil
        case .badCharacters: return L.t("One line of plain text", "只能是一行普通文字")
        case .taken: return L.t("Name taken", "这个名字有人用了")
        }
    }

    private func save() {
        guard check == .ok, !model.sendFlow.isSending else {
            if check == .unchanged { model.mossCardRenaming = nil }
            return
        }
        let new = name.trimmingCharacters(in: .whitespaces)
        model.mossCardRenaming = nil
        model.sendMessage(session, text: Message.renameText(new), done: L.t("Renaming to \(new)…", "正在改名为 \(new)…"))
    }

    private var stateIcon: some View {
        let color = session.needsYou ? Theme.brand : Theme.color(session.state)
        let label = session.needsYou ? L.t("Needs you", "需要你确认") : session.state.label
        return ZStack {
            RoundedRectangle(cornerRadius: 5)
                .fill(color.opacity(scheme == .dark ? 0.22 : 0.14))
            if session.state == .working && !session.needsYou {
                WorkingRing().scaleEffect(0.85)
            } else {
                Image(systemName: session.needsYou ? Icons.needsYou : Icons.state(session.state))
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(color)
            }
        }
        .frame(width: 20, height: 20)
        .tip(label)
        .accessibilityLabel(label)
    }
}

/// A small icon button on a card.
private struct CardIconButton: View {
    let icon: String
    let help: String
    var lit = false
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(lit ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(hovering ? .secondary : .tertiary))
                .frame(width: 22, height: 22)
                .background(RoundedRectangle(cornerRadius: 5).fill(Color.primary.opacity(hovering || lit ? 0.07 : 0)))
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { hovering = $0 }
        .handCursor()
        .tip(help)
        .accessibilityLabel(help)
    }
}

/// The card's main button, in words.
private struct CardTextButton: View {
    let title: String
    let icon: String
    /// Waits (a busy session): drawn faint; a click says why.
    var dimmed = false
    /// Its confirm line is open below.
    var lit = false
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            HStack(spacing: 4) {
                Image(systemName: icon).font(.system(size: 10, weight: .semibold))
                Text(title).font(.system(size: 11.5, weight: .medium))
            }
            .foregroundStyle(Theme.brand)
            .opacity(dimmed ? 0.45 : 1)
            .padding(.horizontal, 8)
            .frame(height: 22)
            .background(Capsule().fill(Theme.brand.opacity(dimmed ? 0.05 : lit ? 0.24 : hovering ? 0.2 : (scheme == .dark ? 0.16 : 0.1))))
            .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { hovering = $0 }
        .handCursor()
    }
}
