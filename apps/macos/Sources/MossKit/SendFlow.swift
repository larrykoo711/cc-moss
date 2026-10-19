import Foundation

/// Sending a message to a session from the panel (`cc-moss send`): one at a
/// time. While it's on its way the form is locked; if it fails, the text
/// stays and says why.
public enum SendFlow: Equatable, Sendable {
    case idle
    case sending
    case failed(String)

    public init() { self = .idle }

    public var isSending: Bool { self == .sending }

    public var failure: String? {
        if case let .failed(why) = self { return why }
        return nil
    }

    /// Start a send. False, and nothing changes, while one is under way.
    public mutating func begin() -> Bool {
        guard !isSending else { return false }
        self = .sending
        return true
    }

    /// It went (nil), or it didn't, and why.
    public mutating func finish(error: String?) {
        self = error.map(SendFlow.failed) ?? .idle
    }
}
