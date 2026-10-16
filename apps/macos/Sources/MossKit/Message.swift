import Foundation

/// Typing into a session from the panel: `cc-moss send <target> -- <text>`.
/// A message is whatever you would type in its terminal; renaming is the
/// same thing with Claude Code's own `/rename` (names come from Claude Code,
/// R7), so the hook then syncs the new name everywhere.
public enum Message {
    public static func arguments(target: String, text: String) -> [String] {
        ["send", target, "--", text]
    }

    /// Only a running session in tmux can be typed into.
    public static func allowed(_ s: Session) -> Bool {
        s.inTmux && (s.state == .working || s.state == .waiting)
    }

    public static func renameText(_ name: String) -> String {
        "/rename " + name.trimmingCharacters(in: .whitespaces)
    }

    public enum RenameCheck: Equatable {
        case ok
        case unchanged
        case empty
        case badCharacters
        /// Another open session already has it (names are unique, T1.5).
        case taken(String)
    }

    public static func checkRename(_ raw: String, of session: Session, among sessions: [Session]) -> RenameCheck {
        let name = raw.trimmingCharacters(in: .whitespaces)
        if name.isEmpty { return .empty }
        if name.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) || $0 == "\n" }) {
            return .badCharacters
        }
        if name == session.name { return .unchanged }
        if let other = sessions.first(where: { $0.id != session.id && $0.state != .closed && $0.name == name }) {
            return .taken(other.title)
        }
        return .ok
    }
}

extension MossClient {
    public func send(target: String, text: String) async throws {
        _ = try await plainData(Message.arguments(target: target, text: text))
    }
}
