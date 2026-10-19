import Foundation

/// Why a running session must restart to use the provider you switched to
/// (`ls --json`'s `provider_restart`).
public enum ProviderRestart: Equatable, Sendable {
    /// Something it was started with is gone from the settings (switching
    /// back to the subscription, typically): the running process keeps it.
    case removed
    /// The pinned model changed.
    case model
    /// A reason this app doesn't know yet: still a restart.
    case other(String)

    public init(_ raw: String) {
        switch raw {
        case "removed": self = .removed
        case "model": self = .model
        default: self = .other(raw)
        }
    }

    /// For the mark's tooltip.
    public var explanation: String {
        switch self {
        case .model: L.t("The provider now picks another model. Restart to use it.",
                         "供应商换了模型，重开才会用上。")
        case .removed, .other: L.t("It still uses the old provider. Restart to switch.",
                                   "它还在用之前的供应商，重开才会换过来。")
        }
    }
}

/// One `provider-changed` event: Claude Code's settings now point elsewhere.
public struct ProviderChange: Equatable, Sendable {
    public static let kind = "provider-changed"

    /// The new base URL's host; nil for the default provider.
    public var host: String?
    /// Every running session picks it up with its next message.
    public var live: Bool
    /// Sessions (slot ids) that must restart to use it.
    public var restartNeeded: [String]

    public init(host: String?, live: Bool, restartNeeded: [String] = []) {
        self.host = host
        self.live = live
        self.restartNeeded = restartNeeded
    }

    /// Some session can't pick it up without a restart.
    public var needsRestart: Bool { !live || !restartNeeded.isEmpty }
}

/// The line at the top of the panel after a provider switch: shown while
/// the newest switch is under 10 minutes old and you haven't closed it.
public struct ProviderBanner: Equatable, Sendable {
    /// How long a switch stays news.
    public static let window: TimeInterval = 10 * 60

    public enum State: Equatable, Sendable {
        case hidden
        /// "Switched to <host>. Your next message uses it."
        case live(host: String?)
        /// "N sessions need a restart…" and "Restart All".
        case needsRestart(count: Int)
    }

    private struct Seen: Equatable, Sendable {
        var seq: Int
        var at: Date
        var change: ProviderChange
    }

    private var latest: Seen?
    private var dismissed: Int?

    public init() {}

    /// Take new events; the newest switch among them wins. `now` stands in
    /// for an event that doesn't say when it happened.
    public mutating func take(_ events: [SessionEvent], now: Date = L.clock()) {
        for e in events {
            guard let change = e.provider, e.seq > (latest?.seq ?? Int.min) else { continue }
            latest = Seen(seq: e.seq, at: e.at ?? now, change: change)
        }
    }

    /// Closed with its ×: hidden until the next switch.
    public mutating func dismiss() {
        dismissed = latest?.seq
    }

    /// What to show now. `sessions` are the current ones: the count follows
    /// them, so restarted sessions drop out and the line turns back into the
    /// plain "switched" note once none is left.
    public func state(now: Date = L.clock(), sessions: [Session]) -> State {
        guard let latest, latest.seq != dismissed else { return .hidden }
        let age = now.timeIntervalSince(latest.at)
        guard age < Self.window, age > -Self.window else { return .hidden }
        if latest.change.needsRestart {
            let stale = Restart.stale(sessions).count
            if stale > 0 { return .needsRestart(count: stale) }
        }
        return .live(host: latest.change.host)
    }

    public static func text(_ state: State) -> String? {
        switch state {
        case .hidden: nil
        case let .live(host?): L.t("Switched to \(host). Your next message uses it.", "已切到 \(host)，下一条消息就走它")
        case .live(nil): L.t("Switched provider. Your next message uses it.", "已换了供应商，下一条消息就走它")
        case let .needsRestart(n):
            n == 1 ? L.t("1 session needs a restart to use the new provider", "1 个会话要重开才能用上新供应商")
                   : L.t("\(n) sessions need a restart to use the new provider", "\(n) 个会话要重开才能用上新供应商")
        }
    }
}

/// Restarting a session in place (`cc-moss restart`): same tmux pane, same
/// conversation, the new provider.
public enum Restart {
    /// The row offers "Restart": it's flagged, running in tmux, not queued
    /// already, and not Moss (cc-moss restarts Moss itself).
    public static func allowed(_ s: Session) -> Bool {
        s.providerRestart != nil && s.status == "running" && s.inTmux && !s.isMoss && !s.restartQueued && !s.isArchived
    }

    /// Everything "Restart All" covers: flagged running sessions, Moss aside.
    public static func stale(_ sessions: [Session]) -> [Session] {
        sessions.filter { $0.providerRestart != nil && $0.status == "running" && !$0.isMoss }
    }

    /// `restart --json -- <name>…`; `--` keeps a name starting with "-" a name.
    public static func arguments(targets: [String]) -> [String] {
        ["restart", "--json", "--"] + targets
    }

    public static let staleArguments = ["restart", "--stale", "--json"]

    /// A short line for the action bar once it's done.
    public static func summary(_ results: [BatchResult]) -> String {
        if results.count == 1, let r = results.first {
            return switch r.outcome {
            case "restarted": L.t("\(r.name) restarted with the new provider.", "\(r.name) 已重开，用上了新供应商。")
            case "queued": L.t("\(r.name) restarts after this turn.", "\(r.name) 这轮结束后重开。")
            default: refusal(r)
            }
        }
        let done = results.filter { $0.outcome == "restarted" }.count
        let queued = results.filter { $0.outcome == "queued" }.count
        let refused = results.count - done - queued
        var parts: [String] = []
        if done > 0 { parts.append(L.t("Restarted \(done)", "重开了 \(done) 个")) }
        if queued > 0 { parts.append(L.t("\(queued) after this turn", "\(queued) 个这轮结束后重开")) }
        if refused > 0 { parts.append(L.t("\(refused) couldn't", "\(refused) 个没能重开")) }
        if parts.isEmpty { return L.t("Nothing needed a restart.", "没有要重开的会话。") }
        return parts.joined(separator: L.t(" · ", "，"))
    }

    /// Why one wasn't restarted, in words.
    public static func refusal(_ r: BatchResult) -> String {
        switch r.reason {
        case "not-in-tmux": L.t("\(r.name) isn't in tmux. Move it in first.", "\(r.name) 不在 tmux 里，先把它移进去。")
        case "moss": L.t("cc-moss restarts Moss itself.", "Moss 由 cc-moss 自己重开。")
        case "not-running": L.t("\(r.name) isn't running.", "\(r.name) 没在运行。")
        default: L.t("Couldn't restart \(r.name).", "没能重开 \(r.name)。")
        }
    }
}

/// One line of `results` in `cc-moss archive|unarchive|restart --json`.
public struct BatchResult: Decodable, Equatable, Sendable {
    public var name: String
    public var sessionId: String?
    /// `archived`, `unarchived`, `restarted`, `queued`, `refused`.
    public var outcome: String
    public var reason: String?

    public init(name: String, sessionId: String? = nil, outcome: String, reason: String? = nil) {
        self.name = name
        self.sessionId = sessionId
        self.outcome = outcome
        self.reason = reason
    }

    enum CodingKeys: String, CodingKey {
        case name
        case sessionId = "session_id"
        case outcome = "result"
        case reason
    }

    public static func decode(_ data: Data) throws -> [BatchResult] {
        struct Wire: Decodable {
            var schema: Int
            var results: [BatchResult]?
        }
        let wire: Wire
        do {
            wire = try JSONDecoder().decode(Wire.self, from: data)
        } catch {
            throw MossClient.Failure.unreadable("\(error)")
        }
        guard wire.schema <= 1 else { throw MossClient.Failure.newerSchema(wire.schema) }
        return wire.results ?? []
    }
}

extension MossClient {
    /// Run a command that answers with `results` in JSON. A refusal may
    /// exit 1 but still answers, so stdout is read either way.
    func batch(_ args: [String], timeout: Double? = nil) async throws -> [BatchResult] {
        let r: CommandResult
        do {
            r = try await runner.run(executable, args, timeout: timeout ?? self.timeout)
        } catch RunError.timedOut {
            throw Failure.timedOut
        } catch {
            throw Failure.command("\(error)")
        }
        if let results = try? BatchResult.decode(r.stdout), r.status == 0 || !results.isEmpty { return results }
        guard r.status == 0 else {
            throw Failure.command(firstLine(r.stderr) ?? firstLine(String(decoding: r.stdout, as: UTF8.self)) ?? "exit \(r.status)")
        }
        return try BatchResult.decode(r.stdout)
    }

    /// `cc-moss restart --json -- <name>…`: idle ones now, busy ones after
    /// their turn. Each waits up to 10 s for the old process to quit.
    public func restart(_ targets: [String]) async throws -> [BatchResult] {
        try await batch(Restart.arguments(targets: targets), timeout: 30 + 15 * Double(targets.count))
    }

    /// `cc-moss restart --stale --json`: every session flagged for a restart.
    public func restartStale() async throws -> [BatchResult] {
        try await batch(Restart.staleArguments, timeout: 120)
    }
}
