import Foundation

/// What `cc-moss suggest` said, kept so the panel doesn't ask again for
/// nothing (user 2026-10-19). Claude Code makes one suggestion per finished
/// turn and keeps it while the session sits idle, so a session's answer is
/// good until its turn marker changes (its last activity; for Moss, its
/// newest reply) or it gets busy. The suggestion is made a moment after the
/// turn ends, so "none" is asked again 2 s and 6 s after the first try before
/// it's believed. Every entry also lapses 10 minutes after it was read: the
/// marker can't see everything (the box typed into in the terminal, /clear).
/// In memory only.
public struct SuggestionCache: Sendable {
    public enum Lookup: Equatable, Sendable {
        /// Known: the suggestion, or that there's none.
        case known(String?)
        /// Ask `cc-moss suggest` now.
        case ask
        /// "None" so far, too soon to believe it: ask again at this time.
        case askAt(Date)
        /// Busy: nothing to ask for.
        case skip
    }

    public static let lifetime: TimeInterval = 10 * 60
    /// After the first "none", ask again this long after the first try.
    public static let retries: [TimeInterval] = [2, 6]

    struct Entry: Sendable {
        var marker: String
        var value: String?
        var firstAskedAt: Date
        var readAt: Date
        var tries: Int
    }

    private var entries: [String: Entry] = [:]

    public init() {}

    public func lookup(_ key: String, marker: String, busy: Bool, now: Date) -> Lookup {
        if busy { return .skip }
        guard let e = entries[key], e.marker == marker, now.timeIntervalSince(e.readAt) < Self.lifetime else { return .ask }
        if e.value != nil { return .known(e.value) }
        guard e.tries <= Self.retries.count else { return .known(nil) }
        let due = e.firstAskedAt.addingTimeInterval(Self.retries[e.tries - 1])
        return now >= due ? .ask : .askAt(due)
    }

    /// What `cc-moss suggest` just said for `key` at turn `marker`.
    public mutating func store(_ key: String, marker: String, value: String?, now: Date) {
        if var e = entries[key], e.marker == marker, now.timeIntervalSince(e.readAt) < Self.lifetime {
            e.value = value
            e.readAt = now
            e.tries += 1
            entries[key] = e
        } else {
            entries[key] = Entry(marker: marker, value: value, firstAskedAt: now, readAt: now, tries: 1)
        }
    }

    /// The session got busy, or went: forget what it had.
    public mutating func forget(_ key: String) {
        entries[key] = nil
    }

    /// Forget every key but these (sessions that closed or went).
    public mutating func keep(only keys: Set<String>) {
        entries = entries.filter { keys.contains($0.key) }
    }
}

/// Asking `cc-moss suggest` through the cache: a known answer is used, a
/// busy session asks nothing, and "none" is asked again when the cache says
/// (2 s and 6 s after the first try) before it's believed.
public enum SuggestionAsk {
    public static func run(lookup: () -> SuggestionCache.Lookup,
                           ask: () async -> String?,
                           store: (String?) -> Void,
                           forget: () -> Void,
                           waitUntil: (Date) async -> Void,
                           tries: Int = 6) async -> String? {
        for _ in 0..<tries {
            switch lookup() {
            case let .known(value):
                return value
            case .skip:
                forget()
                return nil
            case .ask:
                let value = await ask()
                store(value)
                if value != nil { return value }
            case let .askAt(due):
                await waitUntil(due)
            }
        }
        return nil
    }
}
