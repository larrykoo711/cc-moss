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
        /// It needed you then, and that's behind it now ("✓ Handled HH:mm",
        /// the time when known).
        case handled(at: Date?)
        /// Nothing to say beyond "that was then".
        case past
    }

    /// For each bubble (by its id), the role of each of its cards (by
    /// session id). `resolvedAt`: when the session stopped needing you after
    /// the given time, if known.
    public static func roles(_ groups: [MossGroup],
                             resolvedAt: (String, Date) -> Date? = { _, _ in nil }) -> [String: [String: Role]] {
        var out: [String: [String: Role]] = [:]
        var newerSeen = Set<String>()
        for group in groups.reversed() {
            let states = group.sessionStates
            var roles: [String: Role] = [:]
            for id in group.sessions {
                if newerSeen.insert(id).inserted {
                    roles[id] = .live
                } else if states[id]?.hasPrefix("needs-you") == true {
                    let then = group.last.at ?? .distantPast
                    roles[id] = .receipt(.handled(at: resolvedAt(id, then)))
                } else {
                    roles[id] = .receipt(.past)
                }
            }
            out[group.id] = roles
        }
        return out
    }
}
