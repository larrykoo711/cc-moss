import Foundation

/// The answer of `cc-moss moss status --json` (schema 1): whether Moss is
/// switched on, whether it runs, since when and how often it restarted.
public struct MossStatus: Equatable, Sendable {
    public var enabled: Bool
    public var running: Bool
    public var since: Date?
    public var restarts: Int
    public var sessionId: String?
    /// Whether the cc-moss daemon that starts Moss runs (true when an older
    /// cc-moss does not say).
    public var daemonRunning: Bool
    /// Why the last start failed, while Moss is on and not running.
    public var lastError: String?
    /// What Moss is doing right now (T2.3); worked out from the rest when an
    /// older cc-moss does not say.
    public var activity: MossActivity
    /// Messages from the panel still waiting to be handed to Moss.
    public var queued: Int
    /// When Moss last answered something sent from the panel.
    public var lastReplyAt: Date?
    /// Text from a delivery that didn't go through is still in Moss's input.
    public var inputBusy: Bool

    public init(enabled: Bool, running: Bool, since: Date? = nil, restarts: Int = 0, sessionId: String? = nil,
                daemonRunning: Bool = true, lastError: String? = nil, activity: MossActivity? = nil,
                queued: Int = 0, lastReplyAt: Date? = nil, inputBusy: Bool = false) {
        self.enabled = enabled
        self.running = running
        self.since = since
        self.restarts = restarts
        self.sessionId = sessionId
        self.daemonRunning = daemonRunning
        self.lastError = lastError
        self.activity = activity ?? MossActivity.derived(enabled: enabled, running: running, daemonRunning: daemonRunning)
        self.queued = queued
        self.lastReplyAt = lastReplyAt
        self.inputBusy = inputBusy
    }

    private struct Wire: Decodable {
        var schema: Int
        var enabled: Bool
        var running: Bool
        var since: String?
        var restarts: Int?
        var session_id: String?
        var daemon_running: Bool?
        var last_error: String?
        var activity: String?
        var queued: Int?
        var last_reply_at: String?
        var input_busy: Bool?
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
                          restarts: wire.restarts ?? 0, sessionId: wire.session_id,
                          daemonRunning: wire.daemon_running ?? true,
                          lastError: wire.last_error.flatMap { $0.isEmpty ? nil : $0 },
                          activity: wire.activity.flatMap(MossActivity.init(rawValue:)),
                          queued: max(0, wire.queued ?? 0),
                          lastReplyAt: wire.last_reply_at.flatMap(parseTimestamp),
                          inputBusy: wire.input_busy ?? false)
    }

    /// `Moss · running for 2 hours (restarted once)` / `Moss · off`: the
    /// Moss card's status line.
    public func line(now: Date = L.clock(), lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        guard enabled else { return t("Moss · off", "Moss · 已关闭") }
        guard running else {
            // On but not running: say what it is waiting for, not "starting"
            // for ever.
            if !daemonRunning { return t("Moss · waiting for the daemon", "Moss · 在等守护进程") }
            if lastError != nil { return t("Moss · couldn't start", "Moss · 没能启动") }
            return t("Moss · starting…", "Moss · 正在启动…")
        }
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

extension MossStatus {
    /// The line under the Moss card's status, when there is something to
    /// say about why it isn't running.
    public func problem(lang: Lang = L.lang) -> String? {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        guard enabled, !running else { return nil }
        if !daemonRunning {
            return t("The cc-moss daemon starts Moss, and it isn't running. Start it below.",
                     "Moss 由 cc-moss 守护进程启动，而它现在没在运行。在下面启动它。")
        }
        if let lastError { return t("Last try: \(lastError)", "上次尝试：\(lastError)") }
        return nil
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
