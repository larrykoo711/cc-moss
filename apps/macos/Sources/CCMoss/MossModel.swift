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
        // One read at a time: a refresh during a poll (or the other way
        // round) waits for it and has it go round once more, never alongside.
        if let running = mossLoad {
            mossLoadAgain = true
            await running.value
            return
        }
        let task = Task { @MainActor in
            repeat {
                mossLoadAgain = false
                if let status = try? await client.mossStatus() { mossDesk.update(status: status) }
                guard let chat = try? await client.mossChat(limit: mossLimit) else { break }
                mossLoaded = chat.messages.count
                let newestBefore = mossDesk.chat.last { $0.from == .moss }?.id
                let replies = mossDesk.update(chat: chat)
                // Moss said something new: Claude Code has a new guess at your answer.
                if mossOnScreen, mossDesk.chat.last(where: { $0.from == .moss })?.id != newestBefore { fetchMossSuggestion() }
                if mossOnScreen { mossDesk.markRead() }
                if !replies.isEmpty { onMossReplies?(replies) }
            } while mossLoadAgain
        }
        mossLoad = task
        await task.value
        mossLoad = nil
    }

    /// What the top of the conversation offers.
    var mossTop: MossPaging.Top { MossPaging.top(limit: mossLimit, loaded: mossLoaded) }

    /// "Load earlier": 30 more, up to 100.
    func loadEarlierMoss() async {
        guard let next = MossPaging.next(after: mossLimit) else { return }
        mossLimit = next
        await loadMoss()
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
        noteQuickUse(text)
        // The guess was for what you'd say now; you've said something.
        mossSuggestion = nil
        Task {
            await mossDesk.send(text)
            refresh()
        }
    }

    func chooseForMoss(_ reply: String, on message: String) {
        noteQuickUse(reply)
        Task {
            await mossDesk.choose(reply, on: message)
            refresh()
        }
    }

    func doMoss(_ action: MossAction) {
        noteQuickUse(action.text())
        Task {
            await mossDesk.perform(action)
            refresh()
        }
    }

    /// Ask Claude Code (through cc-moss) what you'd likely say to Moss next:
    /// off the main thread, 1.5 s at most; nothing if it has no guess.
    func fetchMossSuggestion() {
        if let demo = demoMossSuggestion {
            mossSuggestion = demo
            return
        }
        guard let client, let moss else { return }
        let handle = moss.handle
        Task { mossSuggestion = await client.suggest(handle) }
    }

    /// One of the quick bar's items.
    func doQuick(_ item: MossQuickItem) {
        mossQuickOpen = false
        sayToMoss(item.text())
    }

    /// Something was sent: if it's a quick-bar item (by button, the "⋯"
    /// list, a typed slash command or a suggested reply), count the use.
    func noteQuickUse(_ text: String) {
        defer { quickSends += 1 }
        guard let item = MossQuick.item(sent: text) else { return }
        quickUses = MossQuickLog.adding(item.id, at: Date(), to: quickUses)
        if quickPersist { MossQuickLog.save(quickUses) }
    }

    /// What's going on right now, for the quick bar's order.
    var quickContext: MossQuick.Context {
        let messages = mossDesk.messages
        return MossQuick.Context(queueHasItems: queueCount > 0,
                                 anyInterrupted: sessions.contains { $0.state == .interrupted },
                                 usageHot: MossQuick.usageHot(in: messages),
                                 mossPicks: MossQuick.mossPicks(pin: mossDesk.pin, in: messages, now: L.clock()))
    }

    /// The quick bar's items for this account, everything included.
    var quickItems: [MossQuickItem] { MossQuick.items(account: mossDesk.status?.account ?? .unknown) }

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

    func openMoss() {
        guard let moss, canOpenMoss else { return }
        open(moss)
    }
}
