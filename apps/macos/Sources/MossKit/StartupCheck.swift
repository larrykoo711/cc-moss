import Foundation

/// After a restart or a crash: which sessions stopped while the app wasn't
/// looking (A8). The app remembers which sessions were running whenever the
/// list changes; on the next launch, those that are interrupted now are the
/// ones to tell you about. Nothing on the very first launch.
public enum StartupCheck {
    public enum Reminder: Equatable, Sendable {
        case none
        /// One session: the ordinary "it stopped" alert.
        case one(String)
        /// Several: one summary, "N sessions stopped".
        case several([String])
    }

    /// The ids of the sessions running now, to remember for next time.
    public static func running(_ sessions: [Session]) -> Set<String> {
        Set(sessions.filter { $0.state == .working || $0.state == .waiting }.map(\.id))
    }

    /// Running when the app last looked, interrupted now.
    public static func missed(_ sessions: [Session], lastRunning: Set<String>?) -> [Session] {
        guard let lastRunning else { return [] }
        return sessions.filter { $0.state == .interrupted && lastRunning.contains($0.id) }
    }

    public static func reminder(_ sessions: [Session], lastRunning: Set<String>?) -> Reminder {
        let ids = missed(sessions, lastRunning: lastRunning).map(\.id)
        switch ids.count {
        case 0: return .none
        case 1: return .one(ids[0])
        default: return .several(ids)
        }
    }

    /// Where the running set is kept between launches.
    public static let defaultsKey = "lastRunningSessions"

    public static func load(_ defaults: UserDefaults) -> Set<String>? {
        (defaults.array(forKey: defaultsKey) as? [String]).map(Set.init)
    }

    public static func save(_ ids: Set<String>, _ defaults: UserDefaults) {
        defaults.set(ids.sorted(), forKey: defaultsKey)
    }
}
