import Foundation
import MossKit

/// Talking to Moss from the panel (T2.3). Everything goes through the CLI:
/// `moss status --json` and `moss chat --json` to read, `moss say` to send.
extension AppModel {
    /// Read Moss's status and the conversation again: on every refresh, and
    /// every few seconds while the Moss page is on screen (Moss thinking
    /// doesn't touch the state folder). A failure keeps what was shown.
    func loadMoss() async {
        guard let client else { return }
        if let status = try? await client.mossStatus() { mossDesk.update(status: status) }
        guard let chat = try? await client.mossChat() else { return }
        let replies = mossDesk.update(chat: chat)
        if mossOnScreen { mossDesk.markRead() }
        if !replies.isEmpty { onMossReplies?(replies) }
    }

    /// You're looking at the Moss page right now.
    var mossOnScreen: Bool { panelShown && page == .moss }

    /// The rail's "Moss", and where an answer's alert leads.
    func showMoss() {
        page = .moss
        if panelShown { mossDesk.markRead() }
    }

    /// Send something to Moss; the conversation is read again once cc-moss
    /// has it, so the message shows as logged.
    func sayToMoss(_ text: String) {
        Task {
            await mossDesk.send(text)
            refresh()
        }
    }

    func doMoss(_ action: MossAction) {
        Task {
            await mossDesk.perform(action)
            refresh()
        }
    }

    func resendToMoss(_ id: String) {
        Task {
            await mossDesk.resend(id)
            refresh()
        }
    }

    /// Moss's terminal, opened like any session's; false when it can't be.
    var canOpenMoss: Bool {
        guard let moss else { return false }
        return moss.inTmux && moss.state != .interrupted
    }

    func openMoss() -> Bool {
        guard let moss, canOpenMoss else { return false }
        return open(moss)
    }
}
