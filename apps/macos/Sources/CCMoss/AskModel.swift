import Foundation
import MossKit

/// Answering a session's question from the panel or Mo (T1.4). Everything
/// goes through the CLI: `ask list --json` to read, `ask answer … --by
/// desktop` to answer, `ask take --name desktop` to say the app is here.
extension AppModel {
    /// How long a card stays to say how it went.
    static let answeredFor: TimeInterval = 2
    static let lateFor: TimeInterval = 4

    /// Read the questions again: on every refresh (the state folder changed,
    /// the panel opened). A failure shows no cards and no error: the
    /// terminal still has the question.
    func loadAsks() async {
        guard askEnabled, let client, let list = try? await client.asks() else {
            asks = []
            let change = askDesk.reset()
            if !change.isEmpty { onAsks?(change) }
            return
        }
        asks = list
        let change = askDesk.update(list)
        if !change.isEmpty { onAsks?(change) }
    }

    /// The questions still waiting on you, oldest first (the focused one first).
    var openAskCards: [AskCard] {
        askDesk.ordered
    }

    /// Sessions waiting for you to type something, with no question open.
    var waitingForInput: [Session] {
        AskDesk.waitingForInput(sessions, asks: asks)
    }

    /// Every session, Moss included, for naming the one that asked.
    private var everySession: [Session] { (moss.map { [$0] } ?? []) + sessions }

    func askTitle(_ ask: Ask) -> String {
        AskDesk.sessionTitle(ask, sessions: everySession)
    }

    func askSlot(_ ask: Ask) -> String? {
        AskDesk.slot(ask, sessions: everySession)
    }

    /// The open question of the session in `slot`, if it has one.
    func openAsk(forSlot slot: String) -> AskCard? {
        guard let s = everySession.first(where: { $0.id == slot }) else { return nil }
        return askDesk.open.first { $0.ask.sessionId == s.sessionId }
    }

    /// Change a card (a click, a keystroke); when that makes an answer,
    /// send it. The card stays disabled until cc-moss says how it went.
    func act(on id: String, _ body: (inout AskCard) -> AskReply?) {
        guard let reply = askDesk.edit(id, body), let client else { return }
        Task {
            let outcome = await client.answer(id, with: reply)
            askDesk.finish(id, outcome)
            if case .failed = outcome {} else {
                let wait = outcome == .answered ? Self.answeredFor : Self.lateFor
                Task {
                    try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000_000))
                    askDesk.clear(id)
                }
            }
            refresh()
        }
    }

    /// From a notification or Mo: the panel, on this question.
    func focusAsk(_ id: String) {
        guard askDesk.card(id) != nil else { return }
        askDesk.focused = id
        page = .queue
        askQueue.show(.ask(id), in: queueItems)
    }

    /// The "To Do" page's items, in order.
    var queueItems: [AskQueue.Item] {
        AskQueue.items(cards: askDesk.cards, replies: waitingForInput)
    }

    /// How many still wait on you (the rail's badge): sent and answered
    /// cards don't count.
    var queueCount: Int { askDesk.open.count + waitingForInput.count }

    // MARK: - "The app is here"

    /// Start the heartbeat once, with the switch as the CLI's config has it.
    func startTaking() {
        guard taker == nil else { return }
        let taker = AskTaker(enabled: askEnabled) { [weak self] in
            guard let client = self?.client else { return }
            try? await client.take(name: AskTaker.name)
        }
        self.taker = taker
        Task {
            if let value = try? await client?.config("ask.enabled") { askEnabled = value != "false" }
            taker.enabled = askEnabled
            taker.start()
        }
    }

    /// Settings › Notifications: save it for the CLI (and the hook), then
    /// start or stop saying "here".
    func setAskEnabled(_ on: Bool) async throws {
        try await client?.setConfig("ask.enabled", on ? "true" : "false")
        askEnabled = on
        taker?.enabled = on
        refresh()
    }
}
