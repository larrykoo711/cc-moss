import MossKit
import SwiftUI

// The Moss page's "Waiting on you · N" card (F23): everything waiting on
// you, live, pinned above the text field. Not a message: it isn't in the
// history, and it goes when nothing waits.

extension AppModel {
    /// The card's rows, from the same state as the To Do page.
    var waitingRows: [WaitingRow] {
        WaitingCard.rows(cards: askDesk.cards, replies: waitingForInput, sessions: (moss.map { [$0] } ?? []) + sessions)
    }

    /// Every ask and reply the card stands for (rows hide a session's later asks).
    var waitingItems: [AskQueue.Item] {
        askDesk.cards.filter { $0.phase == .choosing }.map { .ask($0.id) } + waitingForInput.map { .reply($0.id) }
    }
}

struct MossWaitingCard: View {
    @Bindable var model: AppModel
    /// Rows opened to the full question or a reply line.
    @State private var opened: Set<String> = []
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let rows = model.waitingRows
        Group {
            if !rows.isEmpty {
                card(rows)
                    .transition(.opacity.combined(with: .move(edge: .bottom)))
            } else if let n = model.waitingReceipt.receipt() {
                receipt(n)
                    .transition(.opacity)
            }
        }
        .padding(.horizontal, 10)
        .padding(.bottom, rows.isEmpty && model.waitingReceipt.receipt() == nil ? 0 : 8)
        .animation(.easeOut(duration: 0.2), value: rows.map(\.id))
        .animation(.easeOut(duration: 0.2), value: model.waitingReceipt)
        .onAppear { model.waitingReceipt.update(model.waitingItems) }
        .onChange(of: model.waitingItems) { _, items in model.waitingReceipt.update(items) }
        .task(id: model.waitingReceipt.endsAt) {
            guard let end = model.waitingReceipt.endsAt else { return }
            try? await Task.sleep(for: .seconds(max(0, end.timeIntervalSinceNow)))
            model.waitingReceipt.expire()
        }
    }

    private func card(_ rows: [WaitingRow]) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Image(systemName: "tray.full")
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                Text(WaitingCard.title(rows))
                    .font(.system(size: 12, weight: .semibold))
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.top, 9)
            .padding(.bottom, 4)
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(Array(rows.enumerated()), id: \.element.id) { i, row in
                        if i > 0 { Rectangle().fill(Theme.hairline(scheme)).frame(height: 1).padding(.leading, 30).padding(.trailing, 12) }
                        WaitingRowView(model: model, row: row, open: opened.contains(row.id)) {
                            if opened.contains(row.id) { opened.remove(row.id) } else { opened.insert(row.id) }
                        }
                        .transition(.opacity)
                    }
                }
            }
            .scrollIndicators(.never)
            .frame(maxHeight: 260)
            .fixedSize(horizontal: false, vertical: true)
        }
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Theme.raised(scheme)))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Theme.brand.opacity(0.28)))
        .shadow(color: .black.opacity(scheme == .dark ? 0.3 : 0.07), radius: 6, y: 2)
    }

    private func receipt(_ n: Int) -> some View {
        HStack(spacing: 5) {
            Image(systemName: "checkmark").font(.system(size: 10, weight: .bold))
            Text(WaitingReceipt.text(n))
        }
        .font(.system(size: 11.5, weight: .medium))
        .foregroundStyle(Color.green.opacity(0.85))
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 6)
    }
}

/// One session waiting on you: who, what it wants, and the answer right there.
private struct WaitingRowView: View {
    @Bindable var model: AppModel
    let row: WaitingRow
    /// The full question, or the reply line, is open under it.
    let open: Bool
    let toggle: () -> Void

    private var askCard: AskCard? {
        if case let .ask(id) = row.item { return model.askDesk.card(id) }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Circle().fill(Theme.brand).frame(width: 7, height: 7)
                    .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + 4 }
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 5) {
                        Text(row.title)
                            .font(.system(size: 12.5, weight: .semibold))
                            .lineLimit(1)
                            .truncationMode(.middle)
                        if row.more > 0 {
                            Text("+\(row.more)")
                                .font(.system(size: 10.5, weight: .semibold))
                                .monospacedDigit()
                                .foregroundStyle(.secondary)
                                .padding(.horizontal, 5)
                                .frame(height: 15)
                                .background(Capsule().fill(Color.primary.opacity(0.07)))
                                .tip(L.t("\(row.more) more after this one", "这之后还有 \(row.more) 个"))
                        }
                    }
                    Text(row.line)
                        .font(.system(size: 12, design: isPermission ? .monospaced : .default))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .tip(row.line)
                }
                Spacer(minLength: 6)
                QuietButton(title: L.t("Open", "打开"), icon: Icons.open) { openSession() }
            }
            actions
                .padding(.leading, 15)
            if open { detail.padding(.leading, 15) }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private var isPermission: Bool { if case .permission = row.want { true } else { false } }

    @ViewBuilder private var actions: some View {
        switch row.want {
        case .permission:
            HStack(spacing: 6) {
                SoftButton(title: L.t("Deny", "拒绝"), icon: "xmark") { act { $0.deny() } }
                SoftButton(title: L.t("Allow", "允许"), icon: "checkmark", prominent: true) { act { $0.allow() } }
            }
        case let .question(_, options):
            FlowChips(spacing: 6) {
                ForEach(options, id: \.self) { label in
                    OptionChip(title: OptionLabel.split(label).title) { act { $0.choose(label) } }
                }
                SoftButton(title: options.isEmpty ? L.t("Answer…", "回答…") : L.t("Write My Own…", "自己写…"),
                           icon: options.isEmpty ? "list.bullet" : "pencil") {
                    if !options.isEmpty && !open { act { $0.startWriting(); return nil } }
                    toggle()
                }
            }
        case .input:
            SoftButton(title: L.t("Reply", "回一句"), icon: Icons.message) { toggle() }
        }
    }

    @ViewBuilder private var detail: some View {
        if let askCard {
            AskCardBody(card: askCard, sessionTitle: row.title) { body in model.act(on: askCard.id, body) }
        } else if let s = row.session {
            ReplyBox(model: model, session: s)
        }
    }

    private func act(_ body: (inout AskCard) -> AskReply?) {
        guard case let .ask(id) = row.item else { return }
        model.act(on: id, body)
    }

    /// Into the session: its terminal when it lives in tmux, else its row.
    private func openSession() {
        guard let s = row.session else { return }
        if s.inTmux { model.open(s) } else { model.goToSession(s.id) }
    }
}

/// One answer to a question, as a small pill: a click sends it.
private struct OptionChip: View {
    let title: String
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 12, weight: .medium))
                .lineLimit(1)
                .foregroundStyle(hovering ? Theme.brand : .primary)
                .padding(.horizontal, 11)
                .frame(height: 28)
                .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Theme.brand.opacity(hovering ? 0.14 : (scheme == .dark ? 0.10 : 0.06))))
                .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Theme.brand.opacity(hovering ? 0.5 : 0.28)))
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { hovering = $0 }
        .fixedSize()
    }
}

/// Buttons that wrap onto the next line when they don't fit.
private struct FlowChips: Layout {
    var spacing: CGFloat = 6

    init(spacing: CGFloat = 6) { self.spacing = spacing }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0, widest: CGFloat = 0
        for v in subviews {
            let size = v.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > width {
                y += line + spacing
                x = 0
                line = 0
            }
            x += size.width + spacing
            line = max(line, size.height)
            widest = max(widest, x - spacing)
        }
        return CGSize(width: min(widest, width), height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, line: CGFloat = 0
        for v in subviews {
            let size = v.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                y += line + spacing
                x = bounds.minX
                line = 0
            }
            v.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            line = max(line, size.height)
        }
    }
}
