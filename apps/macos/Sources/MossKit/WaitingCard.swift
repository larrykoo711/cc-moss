import Foundation

/// The Moss page's "Waiting on you · N" card (F23): one live card pinned
/// above the text field, not a message. It is built from the same state as
/// the To Do page (open questions and permissions, sessions waiting for a
/// reply), one row per session, oldest first.
public struct WaitingRow: Equatable, Identifiable, Sendable {
    public enum Want: Equatable, Sendable {
        /// A permission: the tool and what it would run or touch.
        case permission(tool: String, detail: String)
        /// A question: its text, and its answers when one click can send one.
        case question(text: String, options: [String])
        /// Its turn is over and it waits for you to type.
        case input
    }

    /// The session's panel id when it's listed, else its Claude session id.
    public var id: String
    /// The session, when it's listed.
    public var session: Session?
    public var title: String
    /// The To Do item this row handles: the session's oldest.
    public var item: AskQueue.Item
    public var want: Want
    /// More asks of the same session behind this one ("+1").
    public var more: Int
    /// Since when it waits.
    public var since: Date

    /// A permission's command is too long for the row's three lines: offer
    /// "Show all" (the whole question card) before anyone allows it.
    public var needsShowAll: Bool {
        guard case let .permission(tool, detail) = want else { return false }
        return Self.overflows("\(tool) · \(detail)")
    }

    /// More than `lines` lines of about `perLine` characters (a monospaced
    /// line in the card is about 46).
    public static func overflows(_ text: String, perLine: Int = 46, lines: Int = 3) -> Bool {
        let rows = text.split(separator: "\n", omittingEmptySubsequences: false)
            .reduce(0) { $0 + max(1, Int(ceil(Double($1.count) / Double(perLine)))) }
        return rows > lines
    }

    /// The row's second line, in plain words.
    public var line: String {
        switch want {
        case let .permission(tool, detail): detail.isEmpty ? tool : "\(tool) · \(detail)"
        case let .question(text, _): text
        case .input: L.t("Waiting for your reply", "等你回话")
        }
    }
}

public enum WaitingCard {
    /// The rows: open asks (not ones being sent or just answered) grouped by
    /// session, the oldest of each shown, then sessions waiting for a reply;
    /// all oldest first.
    public static func rows(cards: [AskCard], replies: [Session], sessions: [Session]) -> [WaitingRow] {
        let open = cards.filter { $0.phase == .choosing }
        var bySession: [String: [AskCard]] = [:]
        var order: [String] = []
        for c in open {
            if bySession[c.ask.sessionId] == nil { order.append(c.ask.sessionId) }
            bySession[c.ask.sessionId, default: []].append(c)
        }
        var rows: [WaitingRow] = order.compactMap { sid in
            let list = bySession[sid]!.sorted { $0.ask.createdAt < $1.ask.createdAt }
            guard let first = list.first else { return nil }
            let s = sessions.first { $0.sessionId == sid }
            return WaitingRow(id: s?.id ?? sid, session: s, title: AskDesk.sessionTitle(first.ask, sessions: sessions),
                              item: .ask(first.id), want: want(first), more: list.count - 1, since: first.ask.createdAt)
        }
        let asking = Set(rows.map(\.id))
        rows += replies.filter { !asking.contains($0.id) }.map {
            WaitingRow(id: $0.id, session: $0, title: $0.title, item: .reply($0.id), want: .input, more: 0, since: $0.lastActiveAt)
        }
        return rows.sorted { ($0.since, $0.id) < ($1.since, $1.id) }
    }

    /// What one ask wants. A question's answers come along only when a click
    /// sends one (a single question, single choice); otherwise the row opens
    /// the full question.
    static func want(_ card: AskCard) -> WaitingRow.Want {
        if card.ask.kind == .permission {
            return .permission(tool: card.ask.tool?.name ?? L.t("A tool", "一个工具"), detail: card.ask.tool?.preview ?? "")
        }
        let q = card.question
        let simple = !card.several && q?.multiSelect == false
        return .question(text: q?.text ?? "", options: simple ? (q?.options.map(\.label) ?? []) : [])
    }

    /// The header: "Waiting on you · N", N the asks and replies (not rows).
    public static func title(_ rows: [WaitingRow]) -> String {
        let n = rows.reduce(0) { $0 + 1 + $1.more }
        return L.t("Waiting on you · \(n)", "等你处理 · \(n)")
    }
}

/// What the card shows after the last row is handled: one quiet line,
/// "✓ Handled N just now", for about 10 seconds or until the next message.
public struct WaitingReceipt: Equatable, Sendable {
    public static let shownFor: TimeInterval = 10

    /// The To Do items the card showed last time, with their session (its
    /// Claude session id).
    private var standing: [AskQueue.Item: String] = [:]
    /// Items that went away since the card last emptied.
    private var handled = 0
    private var shownAt: Date?

    public init() {}

    /// The card's items now, without a session each: every one that goes
    /// counts as handled.
    public mutating func update(_ items: [AskQueue.Item], now: Date = L.clock()) {
        update(Dictionary(items.map { ($0, "") }, uniquingKeysWith: { a, _ in a }), live: nil, now: now)
    }

    /// The card came on screen: take what's there as it is, counting
    /// nothing (what went while you were elsewhere isn't news here).
    public mutating func rebaseline(_ items: [AskQueue.Item: String]) {
        standing = items
        shownAt = nil
        handled = 0
    }

    /// The card's items now, by session. Items that went away count as
    /// handled, unless their session isn't running any more (`live`: the
    /// running sessions; nil counts every one); when none is left, the
    /// receipt line starts. A new item ends it.
    public mutating func update(_ items: [AskQueue.Item: String], live: Set<String>?, now: Date = L.clock()) {
        let now_ = Set(items.keys)
        let gone = standing.filter { !now_.contains($0.key) }
        handled += gone.filter { live?.contains($0.value) ?? true }.count
        if !now_.subtracting(standing.keys).isEmpty, shownAt != nil {
            // Something new after the receipt: a fresh count.
            shownAt = nil
            handled = 0
        }
        standing = items
        if now_.isEmpty {
            if handled > 0 && shownAt == nil { shownAt = now }
        } else {
            shownAt = nil
        }
    }

    /// A message came in the conversation: the receipt goes.
    public mutating func messageArrived() {
        guard shownAt != nil else { return }
        shownAt = nil
        handled = 0
    }

    /// N for "Handled N just now", while it shows.
    public func receipt(now: Date = L.clock()) -> Int? {
        guard let shownAt, now.timeIntervalSince(shownAt) < Self.shownFor else { return nil }
        return handled
    }

    /// When the line goes by itself, while it shows.
    public var endsAt: Date? { shownAt?.addingTimeInterval(Self.shownFor) }

    /// Past its time: forget it (so the next round counts from zero).
    public mutating func expire(now: Date = L.clock()) {
        guard let shownAt, now.timeIntervalSince(shownAt) >= Self.shownFor else { return }
        self.shownAt = nil
        handled = 0
    }

    public static func text(_ n: Int) -> String {
        n == 1 ? L.t("Handled 1 just now", "刚处理了 1 个请求") : L.t("Handled \(n) just now", "刚处理了 \(n) 个请求")
    }
}

extension MossSessionCard {
    /// Whether a mention gets a card under Moss's bubble (F23): only when
    /// there's something to do with it from there (bring it back, move it
    /// into tmux) or to say (it ended, it's gone). One waiting on you is in
    /// the "Waiting on you" card instead; one just running needs nothing.
    public var shown: Bool {
        guard let s = session else { return true }
        if s.isArchived { return !s.isMoss }
        if waiting != nil || s.needsYou { return false }
        switch main {
        case .bringBack, .moveIntoTmux, .unarchive: return true
        case .handle, .open: return false
        case nil: return s.state == .closed
        }
    }
}
