import Foundation

public enum Shell {
    /// Quote a word for a POSIX shell, leaving plainly safe words alone.
    public static func quote(_ s: String) -> String {
        let safe = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_./:@%+=,")
        if !s.isEmpty, s.unicodeScalars.allSatisfy(safe.contains) { return s }
        return "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}

extension Session {
    /// What to hand cc-moss commands: the name (open sessions have unique
    /// names), or the conversation id when there is none. (The slot id is
    /// internal; cc-moss does not resolve it.)
    public var handle: String { target }
}
