import Foundation

/// How much of the conversation the Moss page reads (user 2026-10-19): the
/// last 30 messages, 30 more per "Load earlier", at most 100; older than
/// that, the terminal has it all.
public enum MossPaging {
    public static let page = 30
    public static let most = 100

    /// What the top of the conversation offers.
    public enum Top: Equatable, Sendable {
        /// The whole conversation is loaded: nothing above.
        case nothing
        /// There may be more: "Load earlier".
        case loadEarlier
        /// The page reads no further: "Earlier messages are in the terminal".
        case terminal
    }

    /// The limit after one more "Load earlier" (30 → 60 → 90 → 100); nil at the most.
    public static func next(after limit: Int) -> Int? {
        limit >= most ? nil : min(most, limit + page)
    }

    /// `loaded`: how many messages cc-moss returned for `limit`. Fewer than
    /// asked for means there's nothing older.
    public static func top(limit: Int, loaded: Int) -> Top {
        if loaded < limit { return .nothing }
        return next(after: limit) == nil ? .terminal : .loadEarlier
    }
}

/// Long replies fold (user 2026-10-19): Moss talks like a colleague, and a
/// wall of text waits behind "Show more".
public enum MossFold {
    /// Folded height, in points.
    public static let height: Double = 180
    /// A reply only a little taller isn't worth a fold.
    public static let slack: Double = 36

    public static func folds(_ contentHeight: Double) -> Bool {
        contentHeight > height + slack
    }
}

extension MossDesk {
    /// What the conversation shows: everything but the session digests
    /// (they're for Moss; its reply to one shows like any reply).
    public nonisolated static func visible(_ messages: [MossMessage]) -> [MossMessage] {
        messages.filter { $0.from != .events }
    }
}
