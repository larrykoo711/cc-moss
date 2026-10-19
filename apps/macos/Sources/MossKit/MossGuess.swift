import Foundation

/// Claude Code's guess at what you'd say to Moss, tied to the reply it
/// answers. Asking can take seconds (retries at 2 s and 6 s): an answer that
/// comes back after you've sent something, or after a newer ask, is dropped;
/// and a guess shows only for Moss's newest reply, never while it's thinking.
public struct MossGuess: Equatable, Sendable {
    /// One ask: which reply it was for, and when it was made.
    public struct Ticket: Equatable, Sendable {
        let marker: String
        let generation: Int
    }

    public private(set) var marker: String?
    public private(set) var value: String?
    private var generation = 0

    public init() {}

    /// Start asking for the reply `marker`; older asks no longer count.
    public mutating func begin(marker: String) -> Ticket {
        generation += 1
        return Ticket(marker: marker, generation: generation)
    }

    /// The answer to an ask; ignored when a newer ask or a send came since.
    public mutating func offer(_ value: String?, for ticket: Ticket) {
        guard ticket.generation == generation else { return }
        marker = ticket.marker
        self.value = value
    }

    /// You said something: whatever is still on its way is for an old turn.
    public mutating func sent() {
        generation += 1
        marker = nil
        value = nil
    }

    /// The guess to show: for Moss's newest reply, while Moss isn't thinking.
    public func shown(newestReply: String?, waiting: Bool) -> String? {
        guard !waiting, let value, marker == newestReply else { return nil }
        return value
    }
}
