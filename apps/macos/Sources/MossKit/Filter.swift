import Foundation

public enum Filter {
    /// Projects keeping only sessions that match every word of `query` in their
    /// name, folder, note or last message, ignoring case. Empty query keeps all.
    /// With `state`, only sessions in that state are kept.
    public static func apply(_ query: String, state: SessionState? = nil, to projects: [Project]) -> [Project] {
        let words = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !words.isEmpty || state != nil else { return projects }
        return projects.compactMap { p in
            let kept = p.sessions.filter { s in
                if let state, s.state != state { return false }
                let haystack = [s.title, p.label, p.path, s.notes.note, s.notes.state, s.notes.goal, s.lastPrompt]
                    .compactMap { $0?.lowercased() }
                    .joined(separator: "\n")
                return words.allSatisfy { haystack.contains($0) }
            }
            guard !kept.isEmpty else { return nil }
            var q = p
            q.sessions = kept
            return q
        }
    }
}

/// How many open sessions are in each state, for the summary strip.
public struct StateCounts: Equatable, Sendable {
    public var working = 0
    public var waiting = 0
    public var interrupted = 0

    public init(_ sessions: [Session]) {
        for s in sessions {
            switch s.state {
            case .working: working += 1
            case .waiting: waiting += 1
            case .interrupted: interrupted += 1
            case .closed: break
            }
        }
    }
}
