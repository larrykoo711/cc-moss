import Foundation

/// A session's note from the panel: `cc-moss note <target> -- <text>`.
/// The text is handed over as one argument, never through a shell, so
/// quotes, spaces and a leading "-" arrive exactly as typed. Blank text
/// clears the note (and only the note: goal and state stay).
public enum Note {
    public static func arguments(target: String, text: String) -> [String] {
        let line = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return ["note", target, "--", line]
    }

    /// What the editor starts with: the note the session has now.
    public static func current(_ s: Session) -> String {
        s.notes.note ?? ""
    }
}

extension MossClient {
    public func setNote(target: String, text: String) async throws {
        _ = try await plainData(Note.arguments(target: target, text: text))
    }
}
