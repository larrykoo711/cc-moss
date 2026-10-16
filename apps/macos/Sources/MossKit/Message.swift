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

    /// Up to three names to pick with one click: the folder, the worktree,
    /// a few words of the last prompt. Each is free (a taken one gets the
    /// next free number) and differs from the current name.
    public static func renameSuggestions(for session: Session, among sessions: [Session]) -> [String] {
        var seeds = [slug((session.cwd as NSString).lastPathComponent)]
        if let root = session.worktree?.root { seeds.append(slug((root as NSString).lastPathComponent)) }
        if let prompt = session.lastPrompt { seeds.append(promptSlug(prompt)) }
        var out: [String] = []
        for seed in seeds where !seed.isEmpty {
            guard let name = free(seed, of: session, among: sessions, avoiding: out) else { continue }
            out.append(name)
            if out.count == 3 { break }
        }
        return out
    }

    /// `seed`, or `seed-2`, `seed-3`… whichever is free first.
    private static func free(_ seed: String, of session: Session, among sessions: [Session], avoiding: [String]) -> String? {
        for n in 1...20 {
            let name = n == 1 ? seed : "\(seed)-\(n)"
            if avoiding.contains(name) { return nil }
            switch checkRename(name, of: session, among: sessions) {
            case .ok: return name
            case .unchanged: return nil
            default: continue
            }
        }
        return nil
    }

    /// Lowercase words joined by dashes; anything else is dropped.
    static func slug(_ text: String) -> String {
        text.lowercased()
            .split(whereSeparator: { !($0.isLetter || $0.isNumber) })
            .joined(separator: "-")
    }

    /// The first three meaningful English words of a prompt.
    static func promptSlug(_ prompt: String) -> String {
        let filler: Set<String> = ["the", "a", "an", "to", "of", "and", "or", "for", "in", "on", "please", "can", "you", "it", "this", "that", "my", "me", "with"]
        let words = prompt.lowercased()
            .split(whereSeparator: { !($0.isASCII && ($0.isLetter || $0.isNumber)) })
            .map(String.init)
            .filter { $0.count > 1 && !filler.contains($0) }
        return words.prefix(3).joined(separator: "-")
    }
}

extension MossClient {
    public func send(target: String, text: String) async throws {
        _ = try await plainData(Message.arguments(target: target, text: text))
    }
}
