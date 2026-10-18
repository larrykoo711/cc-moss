import Foundation

/// The answer of `cc-moss moss status --json` (schema 1): whether Moss is
/// switched on, whether it runs, since when and how often it restarted.
public struct MossStatus: Equatable, Sendable {
    public var enabled: Bool
    public var running: Bool
    public var since: Date?
    public var restarts: Int
    public var sessionId: String?

    public init(enabled: Bool, running: Bool, since: Date? = nil, restarts: Int = 0, sessionId: String? = nil) {
        self.enabled = enabled
        self.running = running
        self.since = since
        self.restarts = restarts
        self.sessionId = sessionId
    }

    private struct Wire: Decodable {
        var schema: Int
        var enabled: Bool
        var running: Bool
        var since: String?
        var restarts: Int?
        var session_id: String?
    }

    public static func decode(_ data: Data) throws -> MossStatus {
        let wire: Wire
        do {
            wire = try JSONDecoder().decode(Wire.self, from: data)
        } catch {
            throw MossClient.Failure.unreadable("\(error)")
        }
        guard wire.schema <= 1 else { throw MossClient.Failure.newerSchema(wire.schema) }
        return MossStatus(enabled: wire.enabled, running: wire.running,
                          since: wire.since.flatMap(parseTimestamp),
                          restarts: wire.restarts ?? 0, sessionId: wire.session_id)
    }

    /// `Moss · running for 2 hours (restarted once)` / `Moss · off`: the
    /// Moss card's status line.
    public func line(now: Date = L.clock(), lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        guard enabled else { return t("Moss · off", "Moss · 已关闭") }
        guard running else { return t("Moss · starting…", "Moss · 正在启动…") }
        let base = if let since {
            t("Moss · running for \(DaemonStatus.span(from: since, to: now, lang: lang))",
              "Moss · 运行中 \(DaemonStatus.span(from: since, to: now, lang: lang))")
        } else {
            t("Moss · running", "Moss · 运行中")
        }
        return switch restarts {
        case 0: base
        case 1: base + t(" (restarted once)", "（重启过 1 次）")
        default: base + t(" (restarted \(restarts) times)", "（重启过 \(restarts) 次）")
        }
    }
}

/// What the Moss card knows: not asked yet, the answer, or why there is none.
public enum MossCheck: Equatable, Sendable {
    case checking
    case known(MossStatus)
    case failed(String)

    public var status: MossStatus? {
        if case .known(let s) = self { s } else { nil }
    }

    public func line(now: Date = L.clock(), lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        return switch self {
        case .checking: t("Moss · checking…", "Moss · 正在查看…")
        case .known(let s): s.line(now: now, lang: lang)
        case .failed: t("Moss · couldn't check", "Moss · 查不到状态")
        }
    }
}

extension MossClient {
    /// `cc-moss moss status --json`.
    public func mossStatus() async throws -> MossStatus {
        try MossStatus.decode(Data(try await plain(["moss", "status", "--json"]).utf8))
    }

    /// `cc-moss moss start` / `moss stop`: switch Moss on or off.
    public func setMoss(_ on: Bool) async throws {
        _ = try await plain(["moss", on ? "start" : "stop"], timeout: 20)
    }
}
