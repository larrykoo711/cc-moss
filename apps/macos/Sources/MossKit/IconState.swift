import Foundation

extension Session {
    /// A running session that is asking you something: a permission prompt,
    /// a question, a request for input.
    public var needsYou: Bool {
        status == "running" && attention?.isEmpty == false
    }
}

/// What the menu bar icon shows: the most urgent thing across all sessions.
public enum IconState: Equatable, Sendable, CaseIterable {
    /// Some session is asking you something.
    case needsYou
    /// Some session died and can be brought back.
    case interrupted
    /// Claude is busy somewhere.
    case working
    /// Sessions are open and it is your move in all of them.
    case waiting
    /// Nothing open.
    case idle

    /// The most urgent state wins: needs you > interrupted > working > waiting > idle.
    public init(_ sessions: [Session]) {
        if sessions.contains(where: \.needsYou) {
            self = .needsYou
        } else if sessions.contains(where: { $0.state == .interrupted }) {
            self = .interrupted
        } else if sessions.contains(where: { $0.state == .working }) {
            self = .working
        } else if sessions.contains(where: { $0.state == .waiting }) {
            self = .waiting
        } else {
            self = .idle
        }
    }

    /// For VoiceOver and the icon's tooltip.
    public var label: String {
        switch self {
        case .needsYou: L.t("A session needs you", "有会话需要你确认")
        case .interrupted: L.t("A session was interrupted", "有会话断了")
        case .working: L.t("Claude is working", "Claude 在忙")
        case .waiting: L.t("Waiting for you", "在等你")
        case .idle: L.t("No open sessions", "没有打开的会话")
        }
    }
}

/// The compact number next to the icon (iStat Menus style).
public enum IconBadge {
    /// How many sessions need you; else how many were interrupted; else how
    /// many are working. Nil when all of those are zero: waiting is normal.
    public static func count(_ sessions: [Session]) -> Int? {
        let needsYou = sessions.filter(\.needsYou).count
        if needsYou > 0 { return needsYou }
        let interrupted = sessions.filter { $0.state == .interrupted }.count
        if interrupted > 0 { return interrupted }
        let working = sessions.filter { $0.state == .working }.count
        return working > 0 ? working : nil
    }
}
