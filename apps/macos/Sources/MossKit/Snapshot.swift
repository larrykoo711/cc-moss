import Foundation

/// One Claude Code session as `cc-moss ls --json` reports it (schema 1).
/// Fields the app does not use are left out; unknown fields are ignored.
public struct Session: Decodable, Equatable, Identifiable, Sendable {
    public struct Tmux: Decodable, Equatable, Sendable {
        public var socket: String?
        public var sessionId: String?
        public var sessionName: String?
        public var paneId: String?
    }

    public struct Notes: Decodable, Equatable, Sendable {
        public var goal: String?
        public var state: String?
        public var note: String?

        public init(goal: String? = nil, state: String? = nil, note: String? = nil) {
            self.goal = goal
            self.state = state
            self.note = note
        }
    }

    public struct Worktree: Decodable, Equatable, Sendable {
        public var root: String
        public var repo: String
    }

    public var id: String
    public var name: String?
    public var status: String
    public var sessionId: String
    public var cwd: String
    public var tmux: Tmux?
    public var busy: Bool
    public var lastPrompt: String?
    public var notes: Notes
    public var lastActiveAt: Date
    public var worktree: Worktree?
    /// Why the session is waiting on you right now (`permission`, `question`,
    /// `input`), or nil. Only running sessions carry one.
    public var attention: String?

    enum CodingKeys: String, CodingKey {
        case id, name, status, sessionId, cwd, tmux, busy, lastPrompt, notes, lastActiveAt, worktree, attention
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decodeIfPresent(String.self, forKey: .name)
        status = try c.decode(String.self, forKey: .status)
        sessionId = try c.decode(String.self, forKey: .sessionId)
        cwd = try c.decode(String.self, forKey: .cwd)
        tmux = try c.decodeIfPresent(Tmux.self, forKey: .tmux)
        busy = try c.decodeIfPresent(Bool.self, forKey: .busy) ?? false
        lastPrompt = try c.decodeIfPresent(String.self, forKey: .lastPrompt)
        notes = try c.decodeIfPresent(Notes.self, forKey: .notes) ?? Notes()
        lastActiveAt = try c.decode(Date.self, forKey: .lastActiveAt)
        worktree = try c.decodeIfPresent(Worktree.self, forKey: .worktree)
        attention = try c.decodeIfPresent(String.self, forKey: .attention)
    }
}

/// Everything `cc-moss ls --json` says.
public struct Snapshot: Equatable, Sendable {
    /// The `ls --json` schema this app understands.
    public static let supportedSchema = 1

    public var stateDir: String?
    public var sessions: [Session]

    public init(stateDir: String?, sessions: [Session]) {
        self.stateDir = stateDir
        self.sessions = sessions
    }
}

public enum SnapshotError: Error, Equatable {
    /// The CLI speaks a newer schema than this app.
    case newerSchema(Int)
    case invalid(String)
}

extension Snapshot {
    private struct Wire: Decodable {
        var schema: Int
        var stateDir: String?
        var sessions: [Session]?
    }

    public static func decode(_ data: Data) throws -> Snapshot {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .custom { d in
            let raw = try d.singleValueContainer().decode(String.self)
            guard let date = parseTimestamp(raw) else {
                throw DecodingError.dataCorrupted(.init(codingPath: d.codingPath, debugDescription: "bad time \(raw)"))
            }
            return date
        }
        // Read the schema first: a newer CLI may have changed shapes we
        // would otherwise misreport as corrupt.
        struct Head: Decodable { var schema: Int }
        let head: Head
        do {
            head = try decoder.decode(Head.self, from: data)
        } catch {
            throw SnapshotError.invalid("\(error)")
        }
        if head.schema > supportedSchema {
            throw SnapshotError.newerSchema(head.schema)
        }
        do {
            let wire = try decoder.decode(Wire.self, from: data)
            return Snapshot(stateDir: wire.stateDir, sessions: wire.sessions ?? [])
        } catch {
            throw SnapshotError.invalid("\(error)")
        }
    }
}

/// RFC 3339 as the CLI writes it, with or without fractional seconds.
func parseTimestamp(_ raw: String) -> Date? {
    let withFraction = ISO8601DateFormatter()
    withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    if let d = withFraction.date(from: raw) { return d }
    let plain = ISO8601DateFormatter()
    plain.formatOptions = [.withInternetDateTime]
    return plain.date(from: raw)
}
