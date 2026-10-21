import Foundation

extension Session {
    /// How cc-moss commands that change a session find it: its full
    /// conversation id, which `cc-moss` resolves by prefix. Never the name:
    /// two open sessions can share one, and the command then can't tell
    /// which row was clicked. The slot id in `id` is not something the CLI
    /// looks sessions up by.
    public var target: String { sessionId }
}

/// Interrupted sessions split by how long ago they stopped: the recent ones
/// stay in the list (and in the banner and the filter count); the ones that
/// stopped long ago fold into one "earlier" group, so a pile of old imports
/// does not bury what just broke.
public struct RestoreGroups: Equatable, Sendable {
    /// Older than this and it folds away; the same seven days the event log keeps.
    public static let threshold: TimeInterval = 7 * 24 * 3600

    public var recent: [Session]
    public var older: [Session]

    public init(_ sessions: [Session], now: Date = L.clock(), threshold: TimeInterval = RestoreGroups.threshold) {
        let interrupted = sessions
            .filter { $0.state == .interrupted }
            .sorted { $0.lastActiveAt > $1.lastActiveAt }
        recent = interrupted.filter { !Self.isOld($0, now: now, threshold: threshold) }
        older = interrupted.filter { Self.isOld($0, now: now, threshold: threshold) }
    }

    public static func isOld(_ s: Session, now: Date = L.clock(), threshold: TimeInterval = RestoreGroups.threshold) -> Bool {
        s.state == .interrupted && now.timeIntervalSince(s.lastActiveAt) > threshold
    }

    /// The sessions the main list shows: everything but the folded ones.
    public static func listed(_ sessions: [Session], now: Date = L.clock(), threshold: TimeInterval = RestoreGroups.threshold) -> [Session] {
        sessions.filter { !isOld($0, now: now, threshold: threshold) }
    }
}

/// Why `cc-moss restore --json` could not bring a session back.
public enum RestoreReason: String, Decodable, Equatable, Sendable {
    case empty
    case missingDir = "missing_dir"
    case exitedAtOnce = "exited_at_once"
    case tmuxFailed = "tmux_failed"
    case alreadyRunning = "already_running"
    /// The name matches more than one session (`candidates` lists them).
    case ambiguous
    /// A code from a newer cc-moss.
    case other

    public init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = RestoreReason(rawValue: raw) ?? .other
    }

    /// What the panel says under the session.
    public var explanation: String {
        switch self {
        case .empty: L.t("It never said a word: nothing to bring back.",
                         "这个会话还没说过话，没什么可找回的。")
        case .missingDir: L.t("Its folder is gone: nowhere to come back to.",
                              "它的目录已经不在了，没地方落脚。")
        case .exitedAtOnce: L.t("Claude Code quit right away; try `cc-moss doctor`.",
                                "Claude Code 刚启动就退出了，试试 `cc-moss doctor`。")
        case .tmuxFailed: L.t("tmux couldn't start it.", "tmux 没能把它启动起来。")
        case .alreadyRunning: L.t("It's already running.", "它已经在运行了。")
        case .ambiguous: L.t("More than one session has this name.", "有好几个会话叫这个名字。")
        case .other: L.t("It couldn't be brought back.", "没能把它找回来。")
        }
    }

    /// Trying again will not help: offer "stop reminding" instead.
    public var isFinal: Bool {
        self == .empty || self == .missingDir
    }
}

/// One line of `cc-moss restore --yes --json`.
public struct RestoreResult: Decodable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var ok: Bool
    /// Newer CLIs distinguish an accepted launch from a confirmed SessionStart.
    public var result: String?
    public var reason: RestoreReason?
    /// cc-moss's own words for a failure.
    public var message: String?
    /// The reason exactly as cc-moss wrote it (a newer code included).
    public var reasonCode: String?
    /// For `ambiguous`: the sessions the name could mean.
    public var candidates: [String]

    public init(id: String, name: String, ok: Bool, reason: RestoreReason? = nil, message: String? = nil,
                reasonCode: String? = nil, candidates: [String] = [], result: String? = nil) {
        self.id = id
        self.name = name
        self.ok = ok
        self.result = result
        self.reason = reason
        self.message = message
        self.reasonCode = reasonCode ?? reason.map { $0 == .other ? "other" : $0.rawValue }
        self.candidates = candidates
    }

    public var isRestored: Bool { ok && (result == nil || result == "restored") }
    /// Unknown accepted outcomes remain unconfirmed rather than claiming success.
    public var isStarting: Bool { ok && !isRestored }

    public var recoveryNotice: String? {
        if isRestored { return L.t("\(name) is back. Press ↵ to open it.", "\(name) 找回来了，按 ↵ 打开。") }
        if isStarting { return L.t("\(name) is starting. Waiting for the session to confirm.", "\(name) 正在启动，等待会话确认。") }
        return nil
    }

    enum CodingKeys: String, CodingKey { case id, name, ok, reason, message, candidates, result }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        ok = try c.decode(Bool.self, forKey: .ok)
        result = try c.decodeIfPresent(String.self, forKey: .result)
        reasonCode = (try? c.decodeIfPresent(String.self, forKey: .reason)) ?? nil
        reason = reasonCode.map { RestoreReason(rawValue: $0) ?? .other }
        message = (try? c.decodeIfPresent(String.self, forKey: .message)) ?? nil
        // Names, or objects with a name (and an id): either reads.
        struct Named: Decodable {
            var name: String?
            var id: String?
        }
        if let names = try? c.decodeIfPresent([String].self, forKey: .candidates) {
            candidates = names
        } else if let objects = try? c.decodeIfPresent([Named].self, forKey: .candidates) {
            candidates = objects.compactMap { o in
                switch (o.name, o.id) {
                case let (n?, i?): "\(n) (\(i.prefix(8)))"
                case let (n?, nil): n
                case let (nil, i?): String(i.prefix(8))
                default: nil
                }
            }
        } else {
            candidates = []
        }
    }
}

/// Everything `cc-moss restore --yes --json` says (schema 1).
public struct RestoreReport: Equatable, Sendable {
    public var results: [RestoreResult]

    public init(results: [RestoreResult]) {
        self.results = results
    }

    public var restored: [RestoreResult] { results.filter(\.isRestored) }
    public var starting: [RestoreResult] { results.filter(\.isStarting) }
    public var failed: [RestoreResult] { results.filter { !$0.ok } }

    public static func decode(_ data: Data) throws -> RestoreReport {
        struct Head: Decodable { var schema: Int }
        struct Wire: Decodable { var results: [RestoreResult] }
        let head: Head
        do {
            head = try JSONDecoder().decode(Head.self, from: data)
        } catch {
            throw SnapshotError.invalid("\(error)")
        }
        guard head.schema <= Snapshot.supportedSchema else { throw SnapshotError.newerSchema(head.schema) }
        do {
            return RestoreReport(results: try JSONDecoder().decode(Wire.self, from: data).results)
        } catch {
            throw SnapshotError.invalid("\(error)")
        }
    }
}


/// Honest feedback for a batch: an accepted launch is still pending.
public struct RestoreSummary: Equatable, Sendable {
    public var restored: Int
    public var failed: Int
    public var starting: Int

    public init(restored: Int, failed: Int, starting: Int = 0) {
        self.restored = restored
        self.failed = failed
        self.starting = starting
    }

    public var title: String {
        var parts: [String] = []
        if restored > 0 { parts.append(L.t("Brought back \(restored)", "找回了 \(restored) 个")) }
        if starting > 0 { parts.append(L.t("\(starting) starting…", "\(starting) 个正在启动…")) }
        return parts.isEmpty ? L.t("None came back", "一个都没找回来") : parts.joined(separator: " · ")
    }

    public var confirmed: Bool { restored > 0 && failed == 0 && starting == 0 }
}

/// Bringing sessions back from the panel: which command to run, what is in
/// flight (a second click while it runs does nothing), and what came of it.
public struct RestoreFlow: Equatable, Sendable {
    public enum Scope: Equatable, Sendable {
        /// Every recently interrupted session (the banner's button).
        case all([String])
        /// One session, by its slot id.
        case one(String)
    }

    /// Slot ids being brought back right now.
    public private(set) var inFlight: Set<String> = []
    /// "Bring them all back" is running.
    public private(set) var allInFlight = false
    /// The last failure per slot id, until the session changes or is restored.
    public private(set) var failures: [String: RestoreResult] = [:]
    /// Launches accepted by the CLI, not yet confirmed by a fresh session snapshot.
    public private(set) var starting: [String: RestoreResult] = [:]

    public init() {}

    /// The arguments for cc-moss: `restore --yes --json [-- target…]`. No
    /// targets means every interrupted session.
    public static func arguments(targets: [String]) -> [String] {
        let base = ["restore", "--yes", "--json"]
        return targets.isEmpty ? base : base + ["--"] + targets
    }

    public func isRunning(_ id: String) -> Bool {
        allInFlight || inFlight.contains(id)
    }

    public var busy: Bool { allInFlight || !inFlight.isEmpty }

    /// Mark `scope` as started. False when it is already on its way back:
    /// the caller must not run the command again.
    public mutating func begin(_ scope: Scope) -> Bool {
        switch scope {
        case .one(let id):
            guard !isRunning(id) else { return false }
            inFlight.insert(id)
            failures[id] = nil
            starting[id] = nil
        case .all(let ids):
            guard !busy else { return false }
            allInFlight = true
            inFlight.formUnion(ids)
            for id in ids { failures[id] = nil; starting[id] = nil }
        }
        return true
    }

    /// Record what came back and end `scope`.
    public mutating func finish(_ scope: Scope, report: RestoreReport) {
        for r in report.results {
            failures[r.id] = r.ok ? nil : r
            starting[r.id] = r.isStarting ? r : nil
        }
        end(scope)
    }

    /// The command itself failed (cc-moss said no, or did not answer): every
    /// session in `scope` carries cc-moss's words.
    public mutating func fail(_ scope: Scope, message: String, names: [String: String] = [:]) {
        let ids: [String] = switch scope {
        case .one(let id): [id]
        case .all(let ids): ids
        }
        for id in ids {
            starting[id] = nil
            failures[id] = RestoreResult(id: id, name: names[id] ?? id, ok: false, reason: .other, message: message)
        }
        end(scope)
    }

    /// Forget failures of sessions that are no longer interrupted.
    public mutating func prune(keeping interrupted: Set<String>) {
        failures = failures.filter { interrupted.contains($0.key) }
        starting = starting.filter { interrupted.contains($0.key) }
    }

    private mutating func end(_ scope: Scope) {
        switch scope {
        case .one(let id):
            inFlight.remove(id)
        case .all(let ids):
            allInFlight = false
            inFlight.subtract(ids)
        }
    }
}

extension MossClient {
    /// `cc-moss restore --yes --json [-- target…]`. Launching can take a few
    /// seconds per session (tmux, then Claude Code starting), hence the long wait.
    public func restore(_ targets: [String]) async throws -> RestoreReport {
        let out = try await plainData(RestoreFlow.arguments(targets: targets), timeout: 60)
        do {
            return try RestoreReport.decode(out)
        } catch SnapshotError.newerSchema(let n) {
            throw Failure.newerSchema(n)
        } catch {
            throw Failure.unreadable("\(error)")
        }
    }

    /// Run a command; its stdout, or its error line as a failure.
    func plainData(_ args: [String], timeout: Double? = nil) async throws -> Data {
        let r: CommandResult
        do {
            r = try await runner.run(executable, args, timeout: timeout ?? self.timeout)
        } catch RunError.timedOut {
            throw Failure.timedOut
        } catch {
            throw Failure.command("\(error)")
        }
        guard r.status == 0 else {
            let out = String(decoding: r.stdout, as: UTF8.self)
            throw Failure.command(firstLine(r.stderr) ?? firstLine(out) ?? "exit \(r.status)")
        }
        return r.stdout
    }
}
