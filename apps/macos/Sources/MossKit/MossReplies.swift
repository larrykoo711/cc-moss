import Foundation

/// Telling you Moss answered (T2.3), by the same rule as every alert (A23):
/// one place, once. Only answers to something you sent from the panel; what
/// was already in the log when the app started is never told.
public struct MossReplyWatch: Sendable {
    /// How many ids each list keeps: far more than a conversation page.
    public static let memory = 500

    /// Moss's messages already looked at (the newest `memory`).
    private var seen = RecentSet()
    /// Messages sent from this app, by the id cc-moss gave them.
    private var sentHere = RecentSet()
    /// A first conversation has been read: the history it held is old news.
    private var primed = false

    public init() {}

    /// The panel sent a message and cc-moss gave it this id.
    public mutating func sent(_ id: String) {
        sentHere.insert(id, keeping: Self.memory)
    }

    /// How many ids are remembered (tests: it stays small).
    var remembered: (seen: Int, sent: Int) { (seen.count, sentHere.count) }

    /// The conversation as just read: Moss's new answers to what you sent
    /// from the panel, each one only once.
    public mutating func arrived(_ messages: [MossMessage]) -> [MossMessage] {
        let replies = messages.filter { $0.from == .moss }
        defer {
            primed = true
            for id in replies.map(\.id) { seen.insert(id, keeping: Self.memory) }
        }
        guard primed else { return [] }
        return replies.filter { reply in
            !seen.contains(reply.id) && reply.replyTo.map(sentHere.contains) == true
        }
    }
}

/// A set that keeps only its newest entries.
struct RecentSet: Sendable {
    private var order: [String] = []
    private var members: Set<String> = []

    var count: Int { members.count }

    func contains(_ id: String) -> Bool { members.contains(id) }

    mutating func insert(_ id: String, keeping limit: Int) {
        guard members.insert(id).inserted else { return }
        order.append(id)
        if order.count > limit {
            let drop = order.count - limit
            order.prefix(drop).forEach { members.remove($0) }
            order.removeFirst(drop)
        }
    }
}

/// Where Moss's answer is told: nowhere while you're looking at the Moss
/// page; otherwise where any alert would go (Mo when it's in view, else a
/// notification). Both open the panel on the Moss page.
public enum MossReplyRouter {
    public static func destination(in context: AlertContext, onMossPage: Bool) -> AlertDestination {
        if context.panelOpen && onMossPage { return .none }
        // The panel open on another page doesn't show the answer: tell it as
        // if the panel were closed.
        var elsewhere = context
        elsewhere.panelOpen = false
        return AlertRouter.destination(for: Self.alert, in: elsewhere)
    }

    /// The slot a Moss answer's alert carries: not a session, the Moss page.
    public static let slot = "cc-moss.moss-reply"
    public static let reason = "moss-reply"

    /// Stand-in for the router, which looks only at the context.
    private static let alert = SessionAlert(id: 0, slotId: slot, kind: .mossReply, at: Date(timeIntervalSince1970: 0))

    /// The alert Mo is handed for an answer.
    public static func alert(for reply: MossMessage, id: Int, at: Date) -> SessionAlert {
        SessionAlert(id: id, slotId: slot, kind: .mossReply, name: "Moss", reason: reason, at: at)
    }

    /// The first words of an answer, for a notification.
    public static func excerpt(_ reply: MossMessage, limit: Int = 90, lang: Lang = L.lang) -> String {
        let flat = reply.shownText(lang).split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .joined(separator: " ")
        return flat.count > limit ? String(flat.prefix(limit - 1)) + "…" : flat
    }

    /// The notification for an answer: clicking it opens the panel on the Moss page.
    public static func note(for reply: MossMessage, lang: Lang = L.lang) -> AlertNote {
        AlertNote(id: "cc-moss.moss.\(reply.id)", title: lang == .zh ? "Moss 回你了" : "Moss replied",
                  body: excerpt(reply, lang: lang), action: .open, slotId: slot, thread: slot)
    }
}
