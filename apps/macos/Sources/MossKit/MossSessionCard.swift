import Foundation

/// A session Moss points at in its reply (`"sessions":[…]`), shown as a
/// small card under its words (T2.3 AMEND-4): what it is and what you can do
/// with it from there.
public struct MossSessionCard: Equatable, Identifiable, Sendable {
    /// The card's main button.
    public enum Main: Equatable, Sendable {
        /// Opens it in the terminal (it lives in tmux).
        case open
        /// It was interrupted: bring it back.
        case bringBack
        /// It runs outside tmux: move it in (`cc-moss adopt`), the same move
        /// as its row in the list. A busy one shows it dimmed.
        case moveIntoTmux
        /// It's waiting on you in To Do: go there, on that item.
        case handle(AskQueue.Item)
    }

    /// What it waits on you for, and its place in To Do.
    public struct Waiting: Equatable, Sendable {
        public enum Reason: Equatable, Sendable {
            /// A permission to approve.
            case permission
            /// A question to answer.
            case question
            /// Its turn is over and it waits for you to type.
            case input
        }

        public var reason: Reason
        public var item: AskQueue.Item

        public init(reason: Reason, item: AskQueue.Item) {
            self.reason = reason
            self.item = item
        }
    }

    /// The slot id Moss named.
    public var id: String
    /// Nil when it's gone: no session has this id any more.
    public var session: Session?
    /// Waiting on you in To Do; nil once that's handled.
    public var waiting: Waiting?
    /// Its state when Moss replied (`needs-you-permission`, `working`, …);
    /// nil from an older cc-moss: then it's never shown as handled.
    public var then: String?

    public init(id: String, session: Session?, waiting: Waiting? = nil, then: String? = nil) {
        self.id = id
        self.session = session
        self.waiting = session == nil ? nil : waiting
        self.then = then
    }

    /// It needed you when Moss replied and doesn't now: nothing in To Do for
    /// it and no question on screen. The card says "Handled" and opens again.
    public var resolved: Bool {
        guard let s = session, then?.hasPrefix("needs-you") == true else { return false }
        return waiting == nil && !s.needsYou
    }

    /// Every card needed you when Moss replied, and every one is handled now:
    /// the bubble quiets down and says "All handled".
    public static func allHandled(_ cards: [MossSessionCard]) -> Bool {
        !cards.isEmpty && cards.allSatisfy(\.resolved)
    }

    public var isGone: Bool { session == nil }

    /// Open when it can be opened, "Bring back" when it was interrupted,
    /// "Move into tmux" when it runs outside tmux; nothing for one that ended.
    public var main: Main? {
        guard let s = session else { return nil }
        if let waiting { return .handle(waiting.item) }
        switch s.state {
        case .interrupted: return s.isMoss ? nil : .bringBack
        case .closed: return nil
        case .working, .waiting:
            if s.inTmux { return .open }
            return Adopt.availability(s) == .none ? nil : .moveIntoTmux
        }
    }

    /// Whether the move can go now, or why it waits (busy, waiting on you).
    public var adopt: Adopt.Availability {
        session.map(Adopt.availability) ?? .none
    }

    /// Renaming is a `/rename` message, so it needs a running session (and
    /// Moss's name isn't set that way).
    public var canRename: Bool {
        guard let s = session else { return false }
        return Message.allowed(s) && !s.isMoss
    }

    /// The same pin as the Sessions list; not for one that has ended.
    public var canPin: Bool {
        guard let s = session else { return false }
        return !s.isMoss && s.state != .closed
    }

    /// One card per id Moss named, in its order; ids no session has any more
    /// become "gone" cards. `waiting`: what To Do holds for each, by id.
    public static func cards(_ ids: [String], among sessions: [Session],
                             waiting: [String: Waiting] = [:], then: [String: String] = [:]) -> [MossSessionCard] {
        let byID = Dictionary(sessions.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        return ids.map { MossSessionCard(id: $0, session: byID[$0], waiting: waiting[$0], then: then[$0]) }
    }
}
