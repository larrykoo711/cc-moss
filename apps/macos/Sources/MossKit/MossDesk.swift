import Foundation
import Observation

/// The Moss page's state: the conversation cc-moss keeps, what the panel
/// sent that cc-moss hasn't logged yet, and Moss's status. Sending goes
/// through `say` (`cc-moss moss say`); nothing here touches Moss itself.
@MainActor @Observable
public final class MossDesk {
    /// The conversation as `moss chat --json` last said.
    public private(set) var chat: [MossMessage] = []
    /// Sent from here and not in `chat` yet: on its way, or refused.
    public private(set) var pending: [MossMessage] = []
    public private(set) var status: MossStatus?
    /// A conversation has been read at least once.
    public private(set) var loaded = false
    /// Failed messages already sent again: no second "Resend".
    public private(set) var resent: Set<String> = []
    /// Moss's newest answer you have seen on the page.
    public private(set) var readThrough: String?
    @ObservationIgnored public var watch = MossReplyWatch()
    /// How a message reaches Moss: `cc-moss moss say`, or a stand-in.
    @ObservationIgnored public var say: @MainActor (String) async throws -> String
    @ObservationIgnored private var serial = 0

    public init(say: @escaping @MainActor (String) async throws -> String) {
        self.say = say
    }

    /// Everything to show, oldest first.
    public var messages: [MossMessage] {
        let logged = Set(chat.map(\.id))
        return chat + pending.filter { !logged.contains($0.id) }
    }

    public var activity: MossActivity? { status?.activity }

    /// Moss can take a message now. Unknown status: let cc-moss decide.
    public var canTalk: Bool { status?.activity.canTalk ?? true }

    /// An answer from Moss you haven't seen on the page.
    public var unread: Bool {
        guard let last = chat.last(where: { $0.from == .moss })?.id else { return false }
        return readThrough.map { last > $0 } ?? true
    }

    /// You're looking at the page: everything Moss said so far is read.
    public func markRead() {
        if let last = chat.last(where: { $0.from == .moss })?.id { readThrough = last }
    }

    public func update(status: MossStatus?) {
        if status != self.status { self.status = status }
    }

    /// A fresh read of the conversation. Returns Moss's new answers to what
    /// you sent from the panel (for the alert); the first read returns none
    /// and counts as read.
    @discardableResult
    public func update(chat new: MossChat) -> [MossMessage] {
        let first = !loaded
        loaded = true
        if new.messages != chat { chat = new.messages }
        let logged = Set(chat.map(\.id))
        if pending.contains(where: { logged.contains($0.id) }) { pending.removeAll { logged.contains($0.id) } }
        if first { markRead() }
        return watch.arrived(chat)
    }

    /// Send `text` to Moss. It shows at once as "sending"; cc-moss's answer
    /// turns it into "queued", or "failed" with cc-moss's reason.
    public func send(_ text: String) async {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        serial += 1
        let local = "local-\(serial)"
        pending.append(MossMessage(id: local, at: Date(), from: .you, text: text, delivery: .sending))
        do {
            let id = try await say(text)
            watch.sent(id)
            replace(local) { $0.id = id; $0.delivery = .queued }
        } catch {
            replace(local) { $0.delivery = .failed; $0.error = Self.describe(error) }
        }
    }

    /// One of Moss's abilities: sends its words.
    public func perform(_ action: MossAction, lang: Lang = L.lang) async {
        await send(action.text(lang))
    }

    /// "Resend" under a failed message: the same words again, as a new
    /// message. One refused before reaching cc-moss is replaced.
    public func resend(_ message: MessageID) async {
        guard let m = messages.first(where: { $0.id == message }), m.delivery == .failed else { return }
        if pending.contains(where: { $0.id == message }) {
            pending.removeAll { $0.id == message }
        } else {
            resent.insert(message)
        }
        await send(m.text)
    }

    public typealias MessageID = String

    private func replace(_ id: String, _ change: (inout MossMessage) -> Void) {
        guard let i = pending.firstIndex(where: { $0.id == id }) else { return }
        change(&pending[i])
    }

    static func describe(_ error: Error) -> String {
        switch error {
        case MossClient.Failure.command(let line): line
        case MossClient.Failure.timedOut: L.t("cc-moss took too long to answer.", "cc-moss 太久没回话了。")
        default: L.t("cc-moss couldn't take it.", "cc-moss 没接住这条。")
        }
    }
}
