import Foundation

/// Sessions you pinned to the top of the panel's list (user 2026-10-19):
/// per Mac, by the session's panel id, newest pin first. Pinning only moves
/// a row; its state and filters stay what they are.
public struct Pins: Equatable, Sendable {
    /// Newest pin first.
    public private(set) var ids: [String]

    public init(_ ids: [String] = []) {
        var seen = Set<String>()
        self.ids = ids.filter { seen.insert($0).inserted }
    }

    public func contains(_ id: String) -> Bool { ids.contains(id) }

    /// Pin (to the top of the pinned) or unpin.
    public mutating func toggle(_ id: String) {
        if let i = ids.firstIndex(of: id) {
            ids.remove(at: i)
        } else {
            ids.insert(id, at: 0)
        }
    }

    /// Pins of sessions that are gone or closed are dropped.
    public func pruned(keeping sessions: [Session]) -> Pins {
        let alive = Set(sessions.filter { $0.state != .closed }.map(\.id))
        return Pins(ids.filter(alive.contains))
    }

    /// The pinned sessions in pin order, and everything else (for the
    /// folder groups, which no longer repeat them).
    public func split(_ sessions: [Session]) -> (pinned: [Session], rest: [Session]) {
        let byID = Dictionary(sessions.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let pinned = ids.compactMap { byID[$0] }.filter { $0.state != .closed }
        let pinnedIDs = Set(pinned.map(\.id))
        return (pinned, sessions.filter { !pinnedIDs.contains($0.id) })
    }

    public static let key = "pinnedSessions"

    public static func load(_ defaults: UserDefaults = .standard) -> Pins {
        Pins(defaults.stringArray(forKey: key) ?? [])
    }

    public func save(_ defaults: UserDefaults = .standard) {
        defaults.set(ids, forKey: Self.key)
    }
}
