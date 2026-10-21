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
        /// The provider settings changed; no claim about any session’s model.
        case live(host: String?)
        /// "N sessions need a restart…" and "Restart All"; `outside`: more
        /// that must move into tmux first (no button for them).
        case needsRestart(count: Int, outside: Int = 0)
        case queued(count: Int)
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
    /// them; queued work stays pending until the backend confirms completion.
    public func state(now: Date = L.clock(), sessions: [Session]) -> State {
        guard let latest, latest.seq != dismissed else { return .hidden }
        let age = now.timeIntervalSince(latest.at)
        guard age < Self.window, age > -Self.window else { return .hidden }
        if latest.change.needsRestart {
            let ready = Restart.restartable(sessions).count
            let outside = Restart.outsideTmux(sessions).count
            if ready + outside > 0 { return .needsRestart(count: ready, outside: outside) }
        }
        let queued = sessions.filter { $0.restartQueued && $0.status == "running" && !$0.isMoss && !$0.isArchived }.count
        if queued > 0 { return .queued(count: queued) }
        return .live(host: latest.change.host)
    }

    public static func text(_ state: State) -> String? {
        switch state {
        case .hidden: nil
        case let .live(host?): L.t("Provider settings changed to \(host). Check each session’s model.", "渠道设置已改为 \(host)，请确认各会话的模型")
        case .live(nil): L.t("Provider settings changed. Check each session’s model.", "渠道设置已变更，请确认各会话的模型")
        case let .queued(n): L.t("\(n) sessions queued. You can cancel in each row.", "\(n) 个会话已排队，可在会话行中取消。")
        case let .needsRestart(n, outside):
            [n == 0 ? nil
                : n == 1 ? L.t("1 session needs a restart to use the new provider", "1 个会话要重开才能用上新供应商")
                : L.t("\(n) sessions need a restart to use the new provider", "\(n) 个会话要重开才能用上新供应商"),
             outside == 0 ? nil
                : L.t("\(outside) not in tmux: move \(outside == 1 ? "it" : "them") in first", "\(outside) 个不在 tmux 里，先移进去")]
                .compactMap { $0 }.joined(separator: L.t(". ", "；"))
        }
    }
}

/// Restarting a session in place (`cc-moss restart`): same tmux pane, same
/// conversation. Applying provider defaults is a separate explicit action.
public enum Restart {
    /// Ordinary running tmux sessions can restart without a provider baseline.
    /// Queued, archived and Moss sessions are protected.
    public static func allowed(_ s: Session) -> Bool {
        s.status == "running" && s.inTmux && !s.isMoss && !s.restartQueued && !s.isArchived
    }

    /// Everything "Restart All" covers: flagged running sessions, Moss aside.
    public static func stale(_ sessions: [Session]) -> [Session] {
        sessions.filter { $0.providerRestart != nil && $0.status == "running" && !$0.isMoss }
    }

    /// What "Restart All" can restart now: in tmux and not queued already.
    public static func restartable(_ sessions: [Session]) -> [Session] {
        stale(sessions).filter { $0.inTmux && !$0.restartQueued && !$0.isArchived }
    }

    /// Flagged ones outside tmux: they must move in before a restart.
    public static func outsideTmux(_ sessions: [Session]) -> [Session] {
        stale(sessions).filter { !$0.inTmux && !$0.isArchived }
    }

    /// `restart --json -- <name>…`; `--` keeps a name starting with "-" a name.
    public static func arguments(targets: [String]) -> [String] {
        ["restart", "--json", "--"] + targets
    }

    public static func providerArguments(targets: [String]) -> [String] {
        ["restart", "--provider-default", "--json", "--"] + targets
    }

    public static func cancelArguments(targets: [String]) -> [String] {
        ["restart", "--cancel", "--json", "--"] + targets
    }

    public static let staleArguments = ["restart", "--stale", "--provider-default", "--json"]

    /// A short line for the action bar once it's done.
    public static func summary(_ results: [BatchResult]) -> String {
        if results.count == 1, let r = results.first {
            return switch r.outcome {
            case "restarted": L.t("\(r.name)'s session was restored.", "\(r.name) 的会话已恢复。")
            case "starting": L.t("\(r.name) is restarting…", "\(r.name) 正在重开…")
            case "cancelled": r.reason == "not-queued" ? L.t("No queued operation.", "当前没有排队操作。")
                : L.t("\(r.name)'s queued restart was cancelled.", "\(r.name) 已取消排队重开。")
            case "queued": L.t("\(r.name) is queued.", "\(r.name) 已排队。")
            default: refusal(r)
            }
        }
        let done = results.filter { $0.outcome == "restarted" }.count
        let queued = results.filter { $0.outcome == "queued" }.count
        let starting = results.filter { $0.outcome == "starting" }.count
        let cancelled = results.filter { $0.outcome == "cancelled" && $0.reason != "not-queued" }.count
        let notQueued = results.filter { $0.outcome == "cancelled" && $0.reason == "not-queued" }.count
        let refused = results.count - done - queued - starting - cancelled - notQueued
        var parts: [String] = []
        if done > 0 { parts.append(L.t("Restarted \(done)", "重开了 \(done) 个")) }
        if starting > 0 { parts.append(L.t("\(starting) restarting…", "\(starting) 个正在重开…")) }
        if cancelled > 0 { parts.append(L.t("\(cancelled) cancelled", "\(cancelled) 个已取消")) }
        if notQueued > 0 { parts.append(L.t("\(notQueued) had no queued operation", "\(notQueued) 个没有排队操作")) }
        if queued > 0 { parts.append(L.t("\(queued) queued", "\(queued) 个已排队")) }
        if refused > 0 { parts.append(L.t("\(refused) couldn't", "\(refused) 个没能重开")) }
        if parts.isEmpty { return L.t("Nothing needed a restart.", "没有要重开的会话。") }
        return parts.joined(separator: L.t(" · ", "，"))
    }

    /// Why one wasn't restarted, in words.
    public static func refusal(_ r: BatchResult) -> String {
        if r.reason == "confirmation-stale" {
            return L.t("The pane identity changed. Choose restart again and confirm anew.", "窗格身份已变化，请重新点击重开并确认。")
        }
        if let message = r.message, !message.isEmpty { return message }
        return switch r.reason {
        case "not-in-tmux": L.t("\(r.name) isn't in tmux. Move it in first.", "\(r.name) 不在 tmux 里，先把它移进去。")
        case "moss": L.t("cc-moss restarts Moss itself.", "Moss 由 cc-moss 自己重开。")
        case "not-running": L.t("\(r.name) isn't running.", "\(r.name) 没在运行。")
        case "busy": L.t("The session is busy. Rebuild after this turn.", "会话正在忙，本轮结束后再重建。")
        default: L.t("Couldn't restart \(r.name).", "没能重开 \(r.name)。")
        }
    }
}

/// The exact process identity captured by the backend before asking for consent.
public struct RebuildIdentity: Codable, Equatable, Sendable {
    public let slotId: String
    public let sessionId: String
    public let claudePid: UInt32
    public let claudeStart: String
    public let tmuxSocket: String
    public let paneId: String
    public let panePid: UInt32
    public let paneStart: String

    enum CodingKeys: String, CodingKey {
        case slotId = "slot_id", sessionId = "session_id"
        case claudePid = "claude_pid", claudeStart = "claude_start"
        case tmuxSocket = "tmux_socket", paneId = "pane_id"
        case panePid = "pane_pid", paneStart = "pane_start"
    }

    public var complete: Bool {
        claudePid > 0 && panePid > 0
            && [slotId, sessionId, claudeStart, tmuxSocket, paneId, paneStart]
                .allSatisfy { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }

    public func matches(_ session: Session) -> Bool {
        complete && slotId == session.id && sessionId == session.sessionId
            && tmuxSocket == session.tmux?.socket && paneId == session.tmux?.paneId
    }
}

/// One nonpersistent, single-use confirmation. The initial model policy is frozen.
public struct RebuildPlan: Equatable, Sendable {
    public let identity: RebuildIdentity
    public let applyingProvider: Bool

    public func arguments() throws -> [String] {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let json = String(decoding: try encoder.encode(identity), as: UTF8.self)
        return ["restart", "--rebuild-pane", "--yes", "--rebuild-identity", json]
            + (applyingProvider ? ["--provider-default"] : [])
            + ["--json", "--", identity.sessionId]
    }
}

public struct RebuildGate: Equatable, Sendable {
    public private(set) var pending: RebuildPlan?
    public init() {}

    @discardableResult public mutating func offer(_ result: BatchResult, session: Session, applyingProvider: Bool) -> Bool {
        pending = nil
        guard result.outcome == "refused", result.reason == "pty-proxy",
              let identity = result.rebuildIdentity, identity.matches(session),
              session.status == "running", !session.isMoss, !session.isArchived else { return false }
        pending = RebuildPlan(identity: identity, applyingProvider: applyingProvider)
        return true
    }

    public func canConfirm(_ session: Session) -> Bool {
        pending?.identity.matches(session) == true && Restart.allowed(session) && !session.busy
    }

    public mutating func cancel() { pending = nil }

    /// No refresh or retry inherits this consent, even when the attempt is refused.
    public mutating func confirm(_ session: Session) -> RebuildPlan? {
        defer { pending = nil }
        return canConfirm(session) ? pending : nil
    }
}

/// One line of `results` in `cc-moss archive|unarchive|restart --json`.
public struct BatchResult: Decodable, Equatable, Sendable {
    public var name: String
    public var sessionId: String?
    /// `archived`, `unarchived`, `restarted`, `starting`, `queued`, `cancelled`, `refused`.
    public var outcome: String
    public var reason: String?
    public var message: String?
    public var rebuildIdentity: RebuildIdentity?

    public init(name: String, sessionId: String? = nil, outcome: String, reason: String? = nil, message: String? = nil, rebuildIdentity: RebuildIdentity? = nil) {
        self.name = name
        self.sessionId = sessionId
        self.outcome = outcome
        self.reason = reason
        self.message = message
        self.rebuildIdentity = rebuildIdentity
    }

    enum CodingKeys: String, CodingKey {
        case name
        case sessionId = "session_id"
        case outcome = "result"
        case reason, message
        case rebuildIdentity = "rebuild_identity"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        sessionId = try c.decodeIfPresent(String.self, forKey: .sessionId)
        outcome = try c.decode(String.self, forKey: .outcome)
        reason = try c.decodeIfPresent(String.self, forKey: .reason)
        message = try c.decodeIfPresent(String.self, forKey: .message)
        // A missing or incomplete identity must not hide the original refusal.
        rebuildIdentity = try? c.decodeIfPresent(RebuildIdentity.self, forKey: .rebuildIdentity)
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

    public func applyProvider(_ targets: [String]) async throws -> [BatchResult] {
        try await batch(Restart.providerArguments(targets: targets), timeout: 30 + 15 * Double(targets.count))
    }

    public func rebuildPane(_ plan: RebuildPlan) async throws -> [BatchResult] {
        try await batch(plan.arguments(), timeout: 45)
    }

    public func cancelRestart(_ targets: [String]) async throws -> [BatchResult] {
        try await batch(Restart.cancelArguments(targets: targets))
    }

    /// `cc-moss restart --stale --json`: every session flagged for a restart.
    public func restartStale() async throws -> [BatchResult] {
        try await batch(Restart.staleArguments, timeout: 120)
    }
}
