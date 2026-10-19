import Foundation

/// Reading `ask list` again and again: one that fails (a timeout, a busy
/// moment) changes nothing, so a draft in progress, the alerts already given
/// and Mo's bubble all stay. Only several failures in a row count as the
/// questions being gone.
public struct AskFeed: Equatable, Sendable {
    /// Failures in a row before the questions count as gone.
    public static let failuresBeforeReset = 5

    private var failures = 0

    public init() {}

    /// `list`: what `ask list` said, or nil when it failed. Returns what
    /// changed on the desk, or nil when nothing did.
    public mutating func apply(_ list: [Ask]?, to desk: inout AskDesk) -> AskDesk.Change? {
        guard let list else {
            failures += 1
            guard failures >= Self.failuresBeforeReset else { return nil }
            failures = 0
            return desk.reset()
        }
        failures = 0
        return desk.update(list)
    }
}
