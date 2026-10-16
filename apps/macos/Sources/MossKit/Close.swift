import Foundation

/// "Stop reminding": `cc-moss close`, after which a session is no longer
/// offered for restore (`cc-moss ls --all` still lists it).
public enum Close {
    /// `close -- <target>…`; `--` keeps a name starting with "-" a name.
    public static func arguments(targets: [String]) -> [String] {
        ["close", "--"] + targets
    }

    /// Only an interrupted session can be dismissed: a running one is ended
    /// in Claude Code itself.
    public static func allowed(_ s: Session) -> Bool {
        s.state == .interrupted
    }

    /// The question asked before closing.
    public static func question(_ sessions: [Session]) -> String {
        if sessions.count == 1, let s = sessions.first {
            return L.t("Stop reminding about “\(s.title)”?", "不再提醒「\(s.title)」？")
        }
        return L.t("Stop reminding about these \(sessions.count)?", "这 \(sessions.count) 个会话以后都不再提醒？")
    }
}

/// An action that waits for a second, deliberate yes. `request` only
/// remembers what was asked; nothing runs until `confirm` hands it back.
public struct Confirmation<Value: Equatable & Sendable>: Equatable, Sendable {
    public private(set) var pending: Value?

    public init() {}

    public var asking: Bool { pending != nil }

    public mutating func request(_ value: Value) {
        pending = value
    }

    /// The value to act on now, once; nil when nothing was asked.
    public mutating func confirm() -> Value? {
        defer { pending = nil }
        return pending
    }

    public mutating func cancel() {
        pending = nil
    }
}

extension MossClient {
    /// `cc-moss close -- <target>…`; cc-moss's own words when it says no.
    public func close(_ targets: [String]) async throws {
        _ = try await plainData(Close.arguments(targets: targets))
    }
}
