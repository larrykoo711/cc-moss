import Foundation

extension Session {
    /// How cc-moss commands that change a session find it: its full
    /// conversation id, which `cc-moss` resolves by prefix. Never the name:
    /// two open sessions can share one, and the command then can't tell
    /// which row was clicked. The slot id in `id` is not something the CLI
    /// looks sessions up by.
    public var target: String { sessionId }
}

/// The exact session a restore click meant: its slot and full conversation id,
/// frozen when the click happened. The CLI matches both before anything else and
/// never falls back to a name, so a renamed or renewed slot can't be swapped in.
public struct RestoreTarget: Hashable, Sendable {
    public let slotId: String
    public let sessionId: String

    public init(slotId: String, sessionId: String) {
        self.slotId = slotId
        self.sessionId = sessionId
    }

    public init(_ session: Session) {
        self.init(slotId: session.id, sessionId: session.sessionId)
    }

    /// Same slot, same conversation: what a late answer may still be about.
    public func isSameGeneration(_ session: Session?) -> Bool {
        session?.id == slotId && session?.sessionId == sessionId
    }

    /// Why this target must not be sent now (nil: send it). Checked right before
    /// its own request, so a slot that moved on since the click is skipped,
    /// never retried under its new identity; an unknown source is never
    /// authorised implicitly.
    public func skip(current: Session?, capabilities: CLICapabilities, name: String? = nil) -> RestoreResult? {
        let name = name ?? current?.title ?? slotId
        func refuse(_ reason: RestoreReason, _ message: String? = nil) -> RestoreResult {
            RestoreResult(id: slotId, name: name, ok: false, reason: reason, message: message)
        }
        func outdated(_ message: String) -> RestoreResult {
            RestoreResult(id: slotId, name: name, ok: false, reason: .other, message: message, reasonCode: RestorePolicy.updateRequiredCode)
        }
        guard capabilities.restoresExactly else { return outdated(RestorePolicy.updateRequired) }
        guard capabilities.checksWorkingDirectory else { return outdated(WorkingDirectoryPolicy.updateRequired) }
        guard let current, isSameGeneration(current), !current.isArchived else {
            return refuse(.notFound)
        }
        guard current.state == .interrupted else { return refuse(.alreadyRunning) }
        if let check = current.workingDirectory, !check.ok {
            return refuse(check.reason.flatMap { RestoreReason(rawValue: $0.rawValue) } ?? .other, check.explanation)
        }
        if let plan = current.restoreSource, !plan.isReady {
            return refuse(plan.issue ?? .resumePreflightFailed)
        }
        return nil
    }
}

public enum RestorePolicy {
    /// Reason code for an app-side refusal because the CLI lacks a capability.
    public static let updateRequiredCode = "update_required"

    public static var updateRequired: String {
        L.t("Update cc-moss in Terminal before bringing sessions back. This version can't restore an exact session.",
            "请先更新终端里的 cc-moss；当前版本无法精确找回会话，未发出找回请求。")
    }
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
    case configSourceUnknown = "config_source_unknown"
    case configSourceChanged = "config_source_changed"
    case configSourceUnsupported = "config_source_unsupported"
    case configDirectoryMissing = "config_directory_missing"
    case configNotDirectory = "config_not_directory"
    case configSourceUnreadable = "config_source_unreadable"
    case configSourceUnavailable = "config_source_unavailable"
    case configSettingsInvalid = "config_settings_invalid"
    case invalidConfigSource = "invalid_config_source"
    case resumePreflightFailed = "resume_preflight_failed"
    case cwdForbidden = "cwd_forbidden"
    case cwdMissing = "cwd_missing"
    case cwdNotDirectory = "cwd_not_directory"
    case cwdUnreadable = "cwd_unreadable"
    case cwdUnavailable = "cwd_unavailable"
    case cwdInvalid = "cwd_invalid"
    case cwdChanged = "cwd_changed"
    case alreadyRunning = "already_running"
    /// An exact restore found no current session for that slot and UUID
    /// (it changed generation or is no longer restorable); nothing started.
    case notFound = "not_found"
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
        case .configSourceUnknown: L.t("Choose the configuration for this restore.", "选择本次使用的配置。")
        case .configSourceChanged: L.t("The configuration source changed. Try restoring again to use the current settings.",
                                       "配置来源已更新，请重新发起找回以使用当前设置。")
        case .configSourceUnsupported: L.t("Its startup configuration can't be replayed safely. Check the original launch options.",
                                           "无法安全重放原启动配置，请检查原启动选项。")
        case .configDirectoryMissing: L.t("The settings folder no longer exists. Choose another location.", "原设置文件夹已不存在，请更换位置。")
        case .configNotDirectory: L.t("The settings location isn't a folder. Choose another location.", "设置位置不是文件夹，请更换位置。")
        case .configSourceUnreadable: L.t("The settings location can't be read. Check access or choose another location.", "无法读取设置位置，请检查权限或更换位置。")
        case .configSourceUnavailable: L.t("The settings can't be checked right now. Try again later.", "暂时无法检查设置，请稍后重试。")
        case .configSettingsInvalid: L.t("The settings file is damaged. Repair it or choose another location.", "设置文件已损坏，请修复文件或更换位置。")
        case .invalidConfigSource: L.t("The selected configuration folder isn't valid. Check that it exists and is accessible.",
                                      "所选配置目录不可用，请检查目录是否存在且可访问。")
        case .resumePreflightFailed: L.t("The restore checks did not pass. Check the details before trying again.",
                                        "找回前检查未通过，请先查看详情。")
        case .cwdForbidden, .cwdMissing, .cwdNotDirectory, .cwdUnreadable, .cwdUnavailable, .cwdInvalid, .cwdChanged:
            (WorkingDirectoryReason(rawValue: rawValue) ?? .other).explanation
        case .alreadyRunning: L.t("It's already running.", "它已经在运行了。")
        case .notFound: L.t("This session has changed. Refresh and try again.", "这条会话已变化，请刷新后再试。")
        case .ambiguous: L.t("More than one session has this name.", "有好几个会话叫这个名字。")
        case .other: L.t("It couldn't be brought back.", "没能把它找回来。")
        }
    }

    public var isLocationProblem: Bool {
        [.configDirectoryMissing, .configNotDirectory, .configSourceUnreadable, .configSettingsInvalid, .invalidConfigSource].contains(self)
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

    /// The line about the slot that was asked for. No line (the session is no
    /// longer restorable, or changed) is a failure, never a quiet success.
    public func about(_ target: RestoreTarget, name: String? = nil) -> RestoreReport {
        RestoreReport(results: [results.first { $0.id == target.slotId }
            ?? RestoreResult(id: target.slotId, name: name ?? target.slotId, ok: false, reason: .notFound)])
    }

    /// Only lines about one of `targets`; a failure is dropped once its slot
    /// holds another conversation, so a late answer never marks the new one.
    public func attributable(to targets: [RestoreTarget], current: (String) -> Session?) -> RestoreReport {
        RestoreReport(results: results.filter { r in
            guard let target = targets.first(where: { $0.slotId == r.id }) else { return false }
            return r.ok || target.isSameGeneration(current(r.id))
        })
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

    /// The arguments for cc-moss: `restore --yes --json --exact-slot-id SLOT -- UUID`.
    /// One target per command; the app never restores by name.
    public static func arguments(exact target: RestoreTarget) -> [String] {
        ["restore", "--yes", "--json", "--exact-slot-id", target.slotId, "--", target.sessionId]
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

/// Local source health reported by the CLI, never inferred from a historical path.
public struct RestoreSourcePlan: Decodable, Equatable, Sendable {
    public var kind: String
    public var status: String
    public var displayPath: String?
    public var defaultPath: String?
    public var revision: String
    public var issue: RestoreReason?

    public var isKnown: Bool { kind == "unset" || kind == "explicit" }
    public var isReady: Bool {
        isKnown && status == "ready" && issue == nil && !revision.isEmpty
            && displayPath.map { $0.hasPrefix("/") && $0.rangeOfCharacter(from: .controlCharacters) == nil } == true
    }
    public var isUnknown: Bool { kind == "unknown" && status == "needs_location" && issue == .configSourceUnknown && !revision.isEmpty }
    public var needsLocation: Bool { status == "needs_location" && issue?.isLocationProblem == true }
}

/// An explicit choice for this launch only; never saved as the old session's source.
public enum RestoreConfigSource: Equatable, Sendable {
    case defaultConfig
    case directory(String)

    public var arguments: [String] {
        switch self {
        case .defaultConfig: ["--config-source", "default"]
        case let .directory(path): ["--config-dir", path]
        }
    }

    public var isValid: Bool {
        switch self {
        case .defaultConfig: true
        case let .directory(path): path.hasPrefix("/") && path.rangeOfCharacter(from: .controlCharacters) == nil
        }
    }
}

public struct RestoreSourceTicket: Equatable, Sendable {
    public let token: UUID
    public let slotId: String
    public let sessionId: String
    public let sourceRevision: String?

    fileprivate init(_ session: Session) {
        token = UUID()
        slotId = session.id
        sessionId = session.sessionId
        sourceRevision = session.restoreSource?.revision
    }

    public var target: RestoreTarget { RestoreTarget(slotId: slotId, sessionId: sessionId) }

    fileprivate func matchesIdentity(_ session: Session?) -> Bool {
        guard let session else { return false }
        return slotId == session.id && sessionId == session.sessionId && session.state == .interrupted && !session.isArchived
    }

    public func matches(_ session: Session?) -> Bool {
        matchesIdentity(session) && sourceRevision == session?.restoreSource?.revision
    }
}

public struct RestoreSourceRequest: Equatable, Sendable {
    public let ticket: RestoreSourceTicket
    public let source: RestoreConfigSource
    public let expectedSourceRevision: String?
    public var arguments: [String] {
        let guardArguments = expectedSourceRevision.map { ["--expected-source-revision", $0] } ?? ["--require-unknown-config"]
        return ["restore", "--yes", "--json", "--exact-slot-id", ticket.slotId] + guardArguments + source.arguments + ["--", ticket.sessionId]
    }
}

public struct RestoreSourceChoice: Equatable, Sendable {
    public var ticket: RestoreSourceTicket
    public var source: RestoreConfigSource?
    public var submitting = false
    public var advanced = false
    public var issue: RestoreReason?
    public var issuePath: String?
    public var canConfirm: Bool { source != nil && !submitting }

    public func offersDefaultProposal(for session: Session) -> Bool {
        guard ticket.matches(session), !advanced, !submitting, source == nil, issue == .configSourceUnknown else { return false }
        guard let plan = session.restoreSource else { return true }
        return plan.isUnknown && plan.defaultPath.map { $0.hasPrefix("/") && $0.rangeOfCharacter(from: .controlCharacters) == nil } == true
    }
}

/// The last unknown-source result offers a choice, not proof of the current source.
/// The backend rechecks its current recorded source before accepting these arguments.
public struct RestoreSourceFlow: Equatable, Sendable {
    public private(set) var choices: [String: RestoreSourceChoice] = [:]
    public private(set) var picker: RestoreSourceTicket?
    public init() {}
    public subscript(_ id: String) -> RestoreSourceChoice? { choices[id] }

    public static func canOffer(_ session: Session, reason: RestoreReason?) -> Bool {
        guard session.state == .interrupted, !session.isArchived else { return false }
        let issue = reason ?? session.restoreSource?.issue
        if issue == .configSourceUnknown {
            return session.restoreSource == nil || session.restoreSource?.isUnknown == true
        }
        guard issue?.isLocationProblem == true, let plan = session.restoreSource else { return false }
        if plan.isUnknown { return true }
        return plan.isKnown && !plan.revision.isEmpty && ["ready", "needs_location"].contains(plan.status)
            && plan.issue != .configSourceUnsupported
    }

    public mutating func offer(_ session: Session, reason: RestoreReason?, failedPath: String? = nil) {
        guard Self.canOffer(session, reason: reason) else {
            cancel(session.id)
            return
        }
        let issue = reason ?? session.restoreSource?.issue
        choices[session.id] = RestoreSourceChoice(ticket: RestoreSourceTicket(session),
                                                issue: issue, issuePath: failedPath)
    }

    public func isCurrent(_ ticket: RestoreSourceTicket, session: Session?) -> Bool {
        choices[ticket.slotId]?.ticket == ticket && ticket.matches(session)
    }

    private func canChoose(_ ticket: RestoreSourceTicket, current: Session?, reason: RestoreReason?) -> Bool {
        guard let current else { return false }
        return isCurrent(ticket, session: current) && choices[ticket.slotId]?.submitting == false
            && Self.canOffer(current, reason: reason ?? choices[ticket.slotId]?.issue)
    }

    public mutating func showAdvanced(_ ticket: RestoreSourceTicket, current: Session?, reason: RestoreReason?) {
        guard canChoose(ticket, current: current, reason: reason) else { return }
        choices[ticket.slotId]?.advanced = true
    }

    @discardableResult public mutating func select(_ source: RestoreConfigSource, ticket: RestoreSourceTicket,
                                                  current: Session?, reason: RestoreReason?) -> Bool {
        guard picker == nil, canChoose(ticket, current: current, reason: reason), source.isValid else { return false }
        if source == .defaultConfig, let plan = current?.restoreSource,
           plan.defaultPath.map({ $0.hasPrefix("/") && $0.rangeOfCharacter(from: .controlCharacters) == nil }) != true { return false }
        choices[ticket.slotId]?.source = source
        return true
    }

    public mutating func beginPicker(current: Session, reason: RestoreReason?) -> RestoreSourceTicket? {
        guard picker == nil, let choice = choices[current.id], canChoose(choice.ticket, current: current, reason: reason) else { return nil }
        let ticket = RestoreSourceTicket(current)
        choices[current.id] = RestoreSourceChoice(ticket: ticket, advanced: true, issue: choice.issue, issuePath: choice.issuePath)
        picker = ticket
        return ticket
    }

    @discardableResult public mutating func receiveDirectory(_ path: String, ticket: RestoreSourceTicket,
                                                            current: Session?, reason: RestoreReason?) -> Bool {
        guard picker == ticket else { return false }
        picker = nil
        return select(.directory(path), ticket: ticket, current: current, reason: reason)
    }

    public mutating func pickerCancelled(_ ticket: RestoreSourceTicket) {
        guard picker == ticket else { return }
        picker = nil
        choices[ticket.slotId] = nil
    }

    public mutating func confirm(_ ticket: RestoreSourceTicket, current: Session?, reason: RestoreReason?) -> RestoreSourceRequest? {
        guard picker == nil, canChoose(ticket, current: current, reason: reason), let source = choices[ticket.slotId]?.source else { return nil }
        choices[ticket.slotId]?.submitting = true
        return RestoreSourceRequest(ticket: ticket, source: source,
                                    expectedSourceRevision: current?.restoreSource?.isKnown == true ? ticket.sourceRevision : nil)
    }

    public mutating func cancel(_ id: String) {
        choices[id] = nil
        if picker?.slotId == id { picker = nil }
    }

    public mutating func observe(_ sessions: [Session]) {
        let current = Dictionary(sessions.map { ($0.id, $0) }, uniquingKeysWith: { _, last in last })
        for (id, choice) in choices {
            guard let session = current[id], choice.ticket.matchesIdentity(session),
                  Self.canOffer(session, reason: choice.issue) else {
                choices[id] = nil
                continue
            }
            if !choice.ticket.matches(session) {
                // A fresh path-error plan can keep the correction card, never an old selection.
                if choice.source == nil, !choice.submitting, choice.issue?.isLocationProblem == true,
                   session.restoreSource?.needsLocation == true {
                    offer(session, reason: session.restoreSource?.issue)
                } else {
                    choices[id] = nil
                }
            }
        }
        if let picker, choices[picker.slotId]?.ticket != picker { self.picker = nil }
    }
}

extension MossClient {
    /// One explicit source choice, exact by slot and UUID.
    public func restore(_ request: RestoreSourceRequest, capabilities: CLICapabilities) async throws -> RestoreReport {
        guard capabilities.restoresExactly else { throw Failure.command(RestorePolicy.updateRequired) }
        return try await restoreReport(request.arguments).about(request.ticket.target)
    }

    /// One session with its recorded source. Launching can take a few seconds
    /// (tmux, then Claude Code starting), hence the long wait.
    public func restore(_ target: RestoreTarget, capabilities: CLICapabilities) async throws -> RestoreReport {
        guard capabilities.restoresExactly else { throw Failure.command(RestorePolicy.updateRequired) }
        return try await restoreReport(RestoreFlow.arguments(exact: target)).about(target)
    }

    /// "Bring them all back": the targets frozen at the click, one exact request
    /// each, in order. `skip` rechecks each right before its turn; a failure is
    /// recorded for its own target and never resent. Not atomic: cancelling
    /// stops before the next request and keeps what already happened.
    public func restoreEach(_ targets: [RestoreTarget], names: [String: String], capabilities: CLICapabilities,
                            skip: @Sendable (RestoreTarget) async -> RestoreResult?) async throws -> RestoreReport {
        guard capabilities.restoresExactly else { throw Failure.command(RestorePolicy.updateRequired) }
        var results: [RestoreResult] = []
        for target in targets {
            if Task.isCancelled { break }
            let refusal = await skip(target)
            if Task.isCancelled { break }
            if let refusal {
                results.append(refusal)
                continue
            }
            let name = names[target.slotId] ?? target.slotId
            do {
                results += try await restoreReport(RestoreFlow.arguments(exact: target)).about(target, name: name).results
            } catch {
                let notice = FailureNotice(action: .restore, subject: nil, error: error)
                results.append(RestoreResult(id: target.slotId, name: name, ok: false,
                                             reason: notice.reason.flatMap(RestoreReason.init(rawValue:)) ?? .other,
                                             message: notice.message ?? notice.why))
            }
        }
        return RestoreReport(results: results)
    }

    private func restoreReport(_ arguments: [String]) async throws -> RestoreReport {
        let out = try await plainData(arguments, timeout: 60)
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
