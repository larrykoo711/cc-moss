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
    }

    /// The slot id Moss named.
    public var id: String
    /// Nil when it's gone: no session has this id any more.
    public var session: Session?

    public init(id: String, session: Session?) {
        self.id = id
        self.session = session
    }

    public var isGone: Bool { session == nil }

    /// Open when it can be opened, "Bring back" when it was interrupted,
    /// "Move into tmux" when it runs outside tmux; nothing for one that ended.
    public var main: Main? {
        guard let s = session else { return nil }
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
    /// become "gone" cards.
    public static func cards(_ ids: [String], among sessions: [Session]) -> [MossSessionCard] {
        let byID = Dictionary(sessions.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        return ids.map { MossSessionCard(id: $0, session: byID[$0]) }
    }
}
