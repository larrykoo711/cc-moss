import MossKit
import SwiftUI

/// A row asking "Move into tmux?": which session, and the name cc-moss will
/// give it (known once its dry run answers).
struct AdoptAsk: Equatable {
    var id: String
    var named: String?
    /// The dry run hasn't answered yet.
    var checking = true
}

// MARK: - Moving a session into tmux (`cc-moss adopt`)

extension AppModel {
    /// "Tidy up and move into tmux" when it has no name or summary yet.
    static func adoptTitle(_ s: Session, named: String? = nil) -> String {
        Adopt.tidies(s, named: named) ? L.t("Tidy up and move into tmux", "整理并移进 tmux")
                                      : L.t("Move into tmux", "移进 tmux")
    }

    /// Why the button waits; nil when it can go.
    static func adoptWait(_ s: Session) -> String? {
        switch Adopt.availability(s) {
        case .working: L.t("It's working; wait for this turn to finish", "它正在干活，等这一轮做完")
        case .waitingOnYou: L.t("It's asking you something; answer it in its terminal first", "它在问你问题，先在它的终端里回答")
        case .ready, .none: nil
        }
    }

    /// The row's button, its "⋯" item or the note's link: ask first, right on
    /// the row. A session that's busy says why it waits instead.
    func askToAdopt(_ s: Session) {
        if let wait = Self.adoptWait(s) {
            showRowNote(s.id, text: wait)
            return
        }
        guard Adopt.availability(s) == .ready, !adopting.contains(s.id) else { return }
        closeReply()
        rowNote = nil
        selectedID = s.id
        adoptProblems[s.id] = nil
        adoptAsk = AdoptAsk(id: s.id)
        guard let client else { return }
        // A dry run: the name it will get, and whether cc-moss would say no.
        Task {
            let outcome = try? await client.adopt(s.target, dryRun: true)
            guard adoptAsk?.id == s.id else { return }
            switch outcome {
            case let .wouldMove(named)?:
                adoptAsk?.named = named
                adoptAsk?.checking = false
            case let .refused(_, message)?:
                adoptAsk = nil
                adoptProblems[s.id] = message
            default:
                // No answer: ask anyway; the move itself will say if it can't.
                adoptAsk?.checking = false
            }
        }
    }

    func cancelAdopt() {
        adoptAsk = nil
    }

    /// "Move into tmux" in the confirm line: the move, then the row is a tmux
    /// row (its new name, its summary), still selected and in view.
    func adopt(_ s: Session) {
        guard let client, !adopting.contains(s.id) else { return }
        adoptAsk = nil
        adoptProblems[s.id] = nil
        adopting.insert(s.id)
        Task {
            do {
                switch try await client.adopt(s.target) {
                case .moved, .wouldMove:
                    follow(s)
                case let .refused(reason, message):
                    adoptProblems[s.id] = message.isEmpty ? L.t("It couldn't be moved.", "没能移进去。") : message
                    report(FailureNotice(action: .adopt, subject: .init(s), reason: reason, message: message))
                case let .failed(message):
                    adoptProblems[s.id] = message.isEmpty ? L.t("It couldn't be moved.", "没能移进去。") : message
                    report(FailureNotice(action: .adopt, subject: .init(s), message: message))
                }
            } catch {
                adoptProblems[s.id] = Self.describe(error)
                report(.adopt, s, error: error)
            }
            adopting.remove(s.id)
            refresh()
        }
    }
}

/// Under the row: what moving does, the name it will get, and the two
/// buttons.
struct AdoptConfirmView: View {
    @Bindable var model: AppModel
    let session: Session
    /// Lined up with the row's text. A card has no inset and shows no keys:
    /// Return and Esc on the Moss page belong to its text field.
    var inset: CGFloat = 44
    var trailing: CGFloat = 10

    private var ask: AdoptAsk? { model.adoptAsk?.id == session.id ? model.adoptAsk : nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            VStack(alignment: .leading, spacing: 3) {
                Text(L.t("This ends its process in its current terminal and carries on in tmux; the conversation is kept.",
                         "会结束它在原终端里的进程，然后在 tmux 里接着跑，对话不会丢。"))
                    .fixedSize(horizontal: false, vertical: true)
                if let named = ask?.named {
                    Text(L.t("It will be named “\(named)”.", "会给它起名「\(named)」。"))
                }
            }
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            HStack(spacing: 8) {
                Spacer(minLength: 0)
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: inset == 0 ? nil : "esc") { model.cancelAdopt() }
                SoftButton(title: AppModel.adoptTitle(session, named: ask?.named), icon: Icons.adopt, prominent: true, shortcut: inset == 0 ? nil : "↵") {
                    model.adopt(session)
                }
            }
        }
        .padding(.leading, inset)
        .padding(.trailing, trailing)
        .padding(.bottom, inset == 0 ? 2 : 10)
    }
}
