import Foundation

public enum RecoveryIntent: Equatable, Sendable {
    case restart, applyProvider, cancelQueue
    case rebuild(RebuildPlan)

    public var kind: RecoveryOperation.Kind {
        switch self {
        case .restart, .applyProvider: .restart
        case .cancelQueue: .cancel
        case .rebuild: .rebuild
        }
    }
}

public struct RecoveryRequest: Equatable, Sendable {
    public let token: UUID
    public let slotId: String
    public let sessionId: String
    public let target: String
    public let intent: RecoveryIntent
}

/// Exactly one presentation per slot. Identity is executable only in confirmation.
public struct RecoveryOperation: Equatable, Sendable {
    public enum Kind: Equatable, Sendable { case restart, rebuild, cancel }
    public enum QueueReason: Equatable, Sendable {
        case busy, inProgress, unknown(String?)
        public init(_ raw: String?) {
            switch raw {
            case "busy": self = .busy
            case "in-progress": self = .inProgress
            default: self = .unknown(raw)
            }
        }
    }
    public enum Stage: Equatable, Sendable {
        case submitting(Kind)
        case confirmRebuild(RebuildPlan)
        case starting, queued, restarted, cancelled, notQueued
        case failed(FailureNotice)
    }

    public let token: UUID
    public let sessionId: String
    public var stage: Stage
    public let kind: Kind
    public let queueReason: QueueReason

    public init(token: UUID, sessionId: String, stage: Stage, kind: Kind = .restart, queueReason: QueueReason = .unknown(nil)) {
        self.token = token
        self.sessionId = sessionId
        self.stage = stage
        self.kind = kind
        self.queueReason = queueReason
    }

    public var isSubmitting: Bool { if case .submitting = stage { true } else { false } }
    public var isFailure: Bool { if case .failed = stage { true } else { false } }
    public var confirmation: RebuildPlan? { if case let .confirmRebuild(plan) = stage { plan } else { nil } }
    public var failure: FailureNotice? { if case let .failed(notice) = stage { notice } else { nil } }

    public var title: String {
        switch stage {
        case .submitting(.restart): L.t("Restart Session", "重开会话")
        case .submitting(.rebuild): L.t("Rebuild Pane", "重建窗格")
        case .submitting(.cancel): L.t("Cancel Queue", "取消排队")
        case .confirmRebuild: L.t("Before Rebuilding", "重建前确认")
        case .starting: L.t("Waiting for Session Confirmation", "等待会话确认")
        case .queued: L.t("Queued", "已排队")
        case .restarted: L.t("Session Restored", "会话已恢复")
        case .cancelled: L.t("Queued Restart Cancelled", "已取消排队")
        case .notQueued: L.t("No Queued Operation", "当前没有排队操作")
        case .failed: kind == .cancel ? L.t("Couldn't Cancel Queue", "未能取消排队") : L.t("Couldn't Restart", "未能重开")
        }
    }

    /// The only place a recovery operation asks its view to draw a spinner.
    public var progressLabel: String? {
        switch stage {
        case .submitting(.restart): L.t("Restarting…", "正在重开…")
        case .submitting(.rebuild): L.t("Rebuilding…", "正在重建…")
        case .submitting(.cancel): L.t("Cancelling…", "正在取消…")
        default: nil
        }
    }

    public var explanation: String? {
        switch stage {
        case .starting: L.t("The launch was accepted, but the session has not confirmed yet.", "启动请求已接受，尚未确认会话恢复。")
        case .queued: queueExplanation(currentBusy: nil)
        case .restarted: L.t("The conversation history is kept. API availability is not verified.", "聊天历史已保留，API 可用性尚未验证。")
        case .cancelled: L.t("This queued restart was cancelled. The session was not restarted.", "本次排队已取消，没有重开会话。")
        case let .failed(notice): notice.recoverySummary
        default: nil
        }
    }

    /// A newer idle snapshot takes precedence over an earlier busy queue reason.
    public func explanation(currentBusy: Bool) -> String? {
        stage == .queued ? queueExplanation(currentBusy: currentBusy) : explanation
    }

    private func queueExplanation(currentBusy: Bool?) -> String? {
        switch queueReason {
        case .busy: currentBusy == false ? nil : L.t("Runs after this turn.", "本轮结束后执行。")
        case .inProgress: L.t("Waiting for another recovery operation.", "正在等待另一项恢复操作。")
        case .unknown: nil
        }
    }

}

/// Nonpersistent operation state, with single-use request tokens for late replies.
public struct RecoveryOperations: Equatable, Sendable {
    public private(set) var bySlot: [String: RecoveryOperation] = [:]
    public init() {}
    public subscript(_ id: String) -> RecoveryOperation? { bySlot[id] }

    public func canConfirm(_ session: Session) -> Bool {
        bySlot[session.id]?.sessionId == session.sessionId
            && bySlot[session.id]?.confirmation?.identity.matches(session) == true
            && Restart.allowed(session) && !session.busy
    }

    public mutating func begin(_ session: Session, intent: RecoveryIntent) -> RecoveryRequest? {
        if let previous = bySlot[session.id], previous.sessionId != session.sessionId {
            bySlot[session.id] = nil
        }
        guard bySlot[session.id]?.isSubmitting != true else { return nil }
        let allowed: Bool
        switch intent {
        case .restart: allowed = Restart.allowed(session)
        case .applyProvider: allowed = ProviderRecovery.canApply(session)
        case .cancelQueue:
            allowed = session.status == "running" && !session.isArchived && !session.isMoss
                && (session.restartQueued || bySlot[session.id]?.stage == .queued)
        case let .rebuild(plan):
            allowed = canConfirm(session) && bySlot[session.id]?.confirmation == plan
        }
        guard allowed else { return nil }
        let request = RecoveryRequest(token: UUID(), slotId: session.id, sessionId: session.sessionId, target: session.target, intent: intent)
        bySlot[session.id] = RecoveryOperation(token: request.token, sessionId: request.sessionId, stage: .submitting(intent.kind), kind: intent.kind)
        return request
    }

    public mutating func confirm(_ session: Session) -> RecoveryRequest? {
        guard bySlot[session.id]?.sessionId == session.sessionId else {
            bySlot[session.id] = nil
            return nil
        }
        guard let plan = bySlot[session.id]?.confirmation else { return nil }
        guard canConfirm(session) else {
            bySlot[session.id] = RecoveryOperation(token: UUID(), sessionId: session.sessionId, stage: .failed(FailureNotice(
                action: .restart, subject: .init(session), reason: session.busy ? "busy" : "confirmation-stale")))
            return nil
        }
        return begin(session, intent: .rebuild(plan))
    }

    public mutating func cancelConfirmation(_ id: String) {
        if bySlot[id]?.confirmation != nil { bySlot[id] = nil }
    }

    public mutating func dismiss(_ id: String) {
        if bySlot[id]?.isSubmitting != true { bySlot[id] = nil }
    }

    /// A full snapshot invalidates deleted/replaced sessions before reconciling queues.
    /// A vanished queue alone is not proof of recovery or cancellation.
    public mutating func observe(_ sessions: [Session]) {
        let identities = Dictionary(sessions.map { ($0.id, $0.sessionId) }, uniquingKeysWith: { _, latest in latest })
        bySlot = bySlot.filter { identities[$0.key] == $0.value.sessionId }
        for session in sessions where !session.restartQueued && bySlot[session.id]?.stage == .queued {
            bySlot[session.id] = nil
        }
    }

    @discardableResult public mutating func complete(_ request: RecoveryRequest, results: [BatchResult], session: Session?) -> Bool {
        guard accepts(request, session: session), let session else { return false }
        guard results.count == 1, let result = results.first else {
            return fail(request, notice: FailureNotice(action: .restart, subject: .init(session),
                reason: "unexpected-result", message: "Expected one restart result."), session: session)
        }
        let stage: RecoveryOperation.Stage
        if result.outcome == "refused" {
            var gate = RebuildGate()
            let canOffer = request.intent == .restart || request.intent == .applyProvider
            if canOffer && gate.offer(result, session: session, applyingProvider: request.intent == .applyProvider), let plan = gate.pending {
                stage = .confirmRebuild(plan)
            } else {
                stage = .failed(FailureNotice(action: .restart, batch: result, subject: .init(session)))
            }
        } else {
            switch (request.intent, result.outcome) {
            case (.cancelQueue, "cancelled") where result.reason == "not-queued": stage = .notQueued
            case (.cancelQueue, "cancelled") where result.reason == nil: stage = .cancelled
            case (.restart, "queued"), (.applyProvider, "queued"): stage = .queued
            case (.restart, "starting"), (.applyProvider, "starting"), (.rebuild, "starting"): stage = .starting
            case (.restart, "restarted"), (.applyProvider, "restarted"), (.rebuild, "restarted"): stage = .restarted
            default:
                stage = .failed(FailureNotice(action: .restart, subject: .init(session),
                    reason: "unexpected-result", message: "Unexpected restart result: \(result.outcome)"))
            }
        }
        bySlot[request.slotId] = RecoveryOperation(token: request.token, sessionId: request.sessionId, stage: stage, kind: request.intent.kind,
            queueReason: stage == .queued ? .init(result.reason) : .unknown(nil))
        return true
    }

    @discardableResult public mutating func fail(_ request: RecoveryRequest, notice: FailureNotice, session: Session?) -> Bool {
        guard accepts(request, session: session) else { return false }
        bySlot[request.slotId] = RecoveryOperation(token: request.token, sessionId: request.sessionId, stage: .failed(notice), kind: request.intent.kind)
        return true
    }

    private mutating func accepts(_ request: RecoveryRequest, session: Session?) -> Bool {
        guard let operation = bySlot[request.slotId], operation.token == request.token,
              operation.isSubmitting else { return false }
        guard operation.sessionId == request.sessionId, let session,
              session.id == request.slotId, session.sessionId == request.sessionId else {
            bySlot[request.slotId] = nil
            return false
        }
        return true
    }
}
