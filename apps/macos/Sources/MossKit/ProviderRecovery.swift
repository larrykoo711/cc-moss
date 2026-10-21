import Foundation

public struct ProviderModel: Decodable, Equatable, Sendable {
    public var model: String?
    public var source: String
    public var revision: String
    public var uncertain: Bool

    public var sourceLabel: String {
        switch source {
        case "env": L.t("Provider environment", "渠道环境设置")
        case "settings": L.t("Provider settings", "渠道设置")
        case "alias": L.t("Resolved alias", "已解析别名")
        case "default": L.t("Provider default", "渠道默认")
        default: L.t("Unknown source", "来源未知")
        }
    }
}

public struct APIFailure: Decodable, Equatable, Sendable {
    public var kind: String
    public var status: Int?
    public var model: String?
    public var atMs: Int64?
}

/// Confirmation records intent, never proof that the provider or API works.
public struct ProviderConfirmations: Equatable, Sendable {
    public private(set) var revisions: [String: String]
    public static let key = "providerModelConfirmations"

    public init(_ revisions: [String: String] = [:]) { self.revisions = revisions }

    public func contains(_ session: Session) -> Bool {
        guard let revision = session.providerModel?.revision, !revision.isEmpty else { return false }
        return revisions[session.id] == revision
    }

    public mutating func confirm(_ session: Session) {
        guard let revision = session.providerModel?.revision, !revision.isEmpty else { return }
        revisions[session.id] = revision
    }

    public static func load(_ defaults: UserDefaults) -> Self {
        Self(defaults.dictionary(forKey: key) as? [String: String] ?? [:])
    }

    public func save(_ defaults: UserDefaults) { defaults.set(revisions, forKey: Self.key) }
}

public enum ProviderRecovery {
    public enum State: Equatable, Sendable {
        case modelUnavailable, apiFailure, queued, changed, uncertain

        public var title: String {
            switch self {
            case .modelUnavailable: L.t("Model unavailable", "模型不可用")
            case .apiFailure: L.t("API request failed", "API 请求失败")
            case .queued: L.t("Queued", "已排队")
            case .changed: L.t("Provider change pending", "待应用渠道")
            case .uncertain: L.t("Check model", "模型待确认")
            }
        }
    }

    public static func state(_ session: Session, confirmations: ProviderConfirmations = .init()) -> State? {
        guard session.status == "running", !session.isArchived, !session.isMoss else { return nil }
        if let failure = session.apiFailure {
            return failure.kind == "model_unavailable" ? .modelUnavailable : .apiFailure
        }
        if session.restartQueued { return .queued }
        if session.providerRestart == .removed { return .changed }
        if confirmations.contains(session) { return nil }
        if session.providerRestart != nil { return .changed }
        if session.providerModel?.uncertain == true { return .uncertain }
        return nil
    }

    public static func repeatsFailedModel(_ session: Session) -> Bool {
        guard let failed = session.apiFailure?.model,
              let target = session.providerModel?.model else { return false }
        return failed == target
    }

    public static func canApplyAll(_ sessions: [Session], confirmations: ProviderConfirmations) -> Bool {
        let targets = Restart.restartable(sessions)
        return !targets.isEmpty && targets.allSatisfy { canApply($0) && !confirmations.contains($0) }
    }

    public static func canKeepSelection(_ session: Session) -> Bool {
        guard session.status == "running", !session.isMoss, !session.isArchived,
              session.apiFailure == nil, !session.restartQueued,
              session.providerModel?.revision.isEmpty == false else { return false }
        return session.providerRestart == .model
            || (session.providerRestart == nil && session.providerModel?.uncertain == true)
    }

    public static func canApply(_ session: Session) -> Bool {
        Restart.allowed(session) && session.providerModel?.model?.isEmpty == false && !repeatsFailedModel(session)
    }
}
