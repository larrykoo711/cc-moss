import Foundation

/// The dim next-prompt suggestion Claude Code shows in a session's own input
/// box, offered in the panel's message form (`cc-moss suggest <handle>`).
/// Shown in place of the placeholder; Tab fills it in; it's never sent
/// unless you fill it in first.
public enum Suggestion {
    /// What the empty field shows: the session's suggestion, else the usual
    /// placeholder. True when it's the suggestion (drawn dim, with "Tab").
    public static func placeholder(text: String, suggestion: String?, fallback: String) -> (text: String, isSuggestion: Bool) {
        if text.isEmpty, let s = usable(suggestion) { return (s, true) }
        return (fallback, false)
    }

    /// Tab (or →) on an empty field: the text to fill in. Nil when you've
    /// typed something (what you typed wins) or there's no suggestion.
    public static func fill(text: String, suggestion: String?) -> String? {
        guard text.isEmpty else { return nil }
        return usable(suggestion)
    }

    static func usable(_ s: String?) -> String? {
        guard let t = s?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty else { return nil }
        return t
    }

    /// `{"schema":1,"suggestion":"…"|null}`; anything else is no suggestion.
    public static func decode(_ data: Data) -> String? {
        struct Body: Decodable {
            var schema: Int
            var suggestion: String?
        }
        guard let body = try? JSONDecoder().decode(Body.self, from: data), body.schema <= 1 else { return nil }
        return usable(body.suggestion)
    }
}

extension MossClient {
    /// What Claude Code suggests typing next in that session, if anything.
    /// Quick or not at all: an older cc-moss, a busy session or a slow
    /// answer all mean no suggestion.
    public func suggest(_ handle: String, timeout: Double = 1.5) async -> String? {
        guard let out = try? await plain(["suggest", handle, "--json"], timeout: timeout) else { return nil }
        return Suggestion.decode(Data(out.utf8))
    }
}
