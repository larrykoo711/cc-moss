import Foundation

/// The conversation is a history: a session card says what was so when Moss
/// replied. Only the newest card of each session is live (its state now, its
/// buttons); older ones freeze into a one-line receipt (user 2026-10-19).
public enum MossCardHistory {
    public enum Role: Equatable, Sendable {
        case live
        case receipt(Receipt)
    }

    public enum Receipt: Equatable, Sendable {
        /// It needed you then, and later it didn't ("✓ Handled").
        case handled
        /// Nothing more can be said than "that was earlier".
        case past
    }

    /// For each bubble (by its id), the role of each of its cards (by
    /// session id). An older card that needed you is "handled" only once a
    /// newer card of that session, or its state now (`needsYouNow`; nil when
    /// unknown), says it no longer does; otherwise it's just "earlier".
    public static func roles(_ groups: [MossGroup],
                             needsYouNow: (String) -> Bool? = { _ in nil }) -> [String: [String: Role]] {
        var out: [String: [String: Role]] = [:]
        var newerSeen = Set<String>()
        /// A later moment (a newer card, or now) where the session didn't need you.
        var laterClear: [String: Bool] = [:]
        for group in groups.reversed() {
            let states = group.sessionStates
            var roles: [String: Role] = [:]
            for id in group.sessions {
                if newerSeen.insert(id).inserted {
                    roles[id] = .live
                    laterClear[id] = needsYouNow(id) == false
                } else if states[id]?.hasPrefix("needs-you") == true, laterClear[id] == true {
                    roles[id] = .receipt(.handled)
                } else {
                    roles[id] = .receipt(.past)
                }
                if let state = states[id], !state.hasPrefix("needs-you") { laterClear[id] = true }
            }
            out[group.id] = roles
        }
        return out
    }
}
