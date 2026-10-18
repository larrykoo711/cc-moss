import Foundation

/// The answer of `cc-moss daemon status --json` (schema 1): whether the
/// resident daemon runs, as which process and version, and since when.
public struct DaemonStatus: Equatable, Sendable {
    public var running: Bool
    public var pid: Int?
    public var version: String?
    public var startedAt: Date?
    public var heartbeatAt: Date?

    public init(running: Bool, pid: Int? = nil, version: String? = nil,
                startedAt: Date? = nil, heartbeatAt: Date? = nil) {
        self.running = running
        self.pid = pid
        self.version = version
        self.startedAt = startedAt
        self.heartbeatAt = heartbeatAt
    }

    public static let stopped = DaemonStatus(running: false)

    private struct Wire: Decodable {
        var schema: Int
        var running: Bool
        var pid: Int?
        var version: String?
        var started_at: String?
        var heartbeat_at: String?
    }

    public static func decode(_ data: Data) throws -> DaemonStatus {
        let wire: Wire
        do {
            wire = try JSONDecoder().decode(Wire.self, from: data)
        } catch {
            throw MossClient.Failure.unreadable("\(error)")
        }
        guard wire.schema <= 1 else { throw MossClient.Failure.newerSchema(wire.schema) }
        return DaemonStatus(running: wire.running, pid: wire.pid, version: wire.version,
                            startedAt: wire.started_at.flatMap(parseTimestamp),
                            heartbeatAt: wire.heartbeat_at.flatMap(parseTimestamp))
    }

    /// `Daemon · running for 3 hours` / `Daemon · not running`: the page's status line.
    public func line(now: Date = L.clock(), lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        guard running else { return t("Daemon · not running", "守护进程 · 没在运行") }
        guard let startedAt else { return t("Daemon · running", "守护进程 · 运行中") }
        let span = Self.span(from: startedAt, to: now, lang: lang)
        return t("Daemon · running for \(span)", "守护进程 · 运行中 \(span)")
    }

    /// `5 minutes`, `3 hours`, `2 days`; under a minute is `less than a minute`.
    public static func span(from start: Date, to now: Date, lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        let s = max(0, Int(now.timeIntervalSince(start)))
        switch s {
        case 0..<60:
            return t("less than a minute", "不到 1 分钟")
        case 60..<3600:
            let n = s / 60
            return t("\(n) minute\(n == 1 ? "" : "s")", "\(n) 分钟")
        case 3600..<86_400:
            let n = s / 3600
            return t("\(n) hour\(n == 1 ? "" : "s")", "\(n) 小时")
        default:
            let n = s / 86_400
            return t("\(n) day\(n == 1 ? "" : "s")", "\(n) 天")
        }
    }
}

/// What the Moss page knows about the daemon: not asked yet, the answer,
/// or why there is none. "Couldn't check" is never shown as "not running".
public enum DaemonCheck: Equatable, Sendable {
    case checking
    case known(DaemonStatus)
    /// cc-moss could not be asked, or did not answer; the reason, for people.
    case failed(String)

    public var status: DaemonStatus? {
        if case .known(let s) = self { s } else { nil }
    }

    /// The page's status line.
    public func line(now: Date = L.clock(), lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        return switch self {
        case .checking: t("Daemon · checking…", "守护进程 · 正在查看…")
        case .known(let s): s.line(now: now, lang: lang)
        case .failed: t("Daemon · couldn't check", "守护进程 · 查不到状态")
        }
    }
}

extension MossClient {
    /// `cc-moss daemon status --json`.
    public func daemonStatus() async throws -> DaemonStatus {
        try DaemonStatus.decode(Data(try await plain(["daemon", "status", "--json"]).utf8))
    }

    /// `cc-moss daemon start`: launchd starts it (setting it up if needed);
    /// returns once it runs or the CLI gives up.
    public func startDaemon() async throws {
        _ = try await plain(["daemon", "start"], timeout: 30)
    }
}
