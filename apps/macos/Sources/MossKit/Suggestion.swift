import Foundation

/// The next-prompt suggestion Claude Code shows in a session's own input box
/// (`cc-moss suggest <handle>`). The panel puts it straight into the field as
/// real text, all selected (user 2026-10-19): Return sends it, typing
/// replaces it. It never overwrites anything you typed.
public enum Suggestion {
    /// The text to put in the field when a suggestion arrives: only into an
    /// empty field you haven't typed in since it opened.
    public static func prefill(text: String, touched: Bool, suggestion: String?) -> String? {
        guard !touched, text.isEmpty else { return nil }
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

/// Answering sessions right in the list (user 2026-10-19): click a session
/// that's waiting for you to type, and a reply line opens under its row.
public enum ReplyLine {
    /// Rows a click opens a reply line under: running, not busy, able to take
    /// typing, not asking for a permission or a choice, and not Moss (which
    /// has its own page).
    public static func opens(_ s: Session) -> Bool {
        guard !s.isMoss, Message.allowed(s), s.state == .waiting else { return false }
        let attention = s.attention ?? ""
        return attention.isEmpty || attention == "input"
    }

    /// Waiting for you to type: the ones a run of replies goes through.
    public static func waiting(_ s: Session) -> Bool {
        opens(s) && s.attention == "input"
    }

    /// What Return sends: the field's text (a filled-in suggestion is just
    /// text), or nothing when it's empty.
    public static func toSend(_ text: String) -> String? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? nil : text
    }

    /// The next session waiting for you after `id`, in list order, wrapping
    /// to the top; nil when there's no other.
    public static func next(after id: String, in list: [Session], backwards: Bool = false) -> Session? {
        let order = backwards ? Array(list.reversed()) : list
        guard let i = order.firstIndex(where: { $0.id == id }) else { return order.first(where: waiting) }
        let rest = order[(i + 1)...] + order[..<i]
        return rest.first(where: waiting)
    }
}
