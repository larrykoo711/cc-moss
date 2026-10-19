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

/// A field a suggestion may be put into (user 2026-10-19). Filled and
/// untouched, it's quiet: grey like a placeholder, selected without a
/// highlight, so typing replaces it and Return sends it as is. Any edit, an
/// arrow key or a click makes it ordinary text.
public struct SuggestionField: Equatable, Sendable {
    /// What was put in, while it's still there.
    public private(set) var filled: String?
    /// Filled and not touched since: drawn grey.
    public private(set) var untouched = false
    /// You typed (or cleared) something: no suggestion goes in after that.
    public private(set) var typed = false

    public init() {}

    /// A suggestion arrived: the text to put in the field, or nil to leave it.
    public mutating func offer(_ suggestion: String?, text: String) -> String? {
        guard let fill = Suggestion.prefill(text: text, touched: typed, suggestion: suggestion) else { return nil }
        filled = fill
        untouched = true
        return fill
    }

    /// The field's text changed. The suggestion going in isn't an edit;
    /// anything else is yours.
    public mutating func changed(to text: String) {
        if untouched, text == filled { return }
        untouched = false
        if text != filled || text.isEmpty { typed = true }
    }

    /// An arrow key or a click in the text: it's yours to edit now.
    public mutating func engaged() {
        untouched = false
    }

    /// Sent: a fresh field, ready for the next suggestion.
    public mutating func reset() {
        self = SuggestionField()
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
    /// Rows a click opens a reply line under: running in tmux (busy is fine:
    /// Claude Code keeps what you type for after this turn), not asking for
    /// a permission or a choice, and not Moss (which has its own page).
    public static func opens(_ s: Session) -> Bool {
        guard !s.isMoss, Message.allowed(s) else { return false }
        let attention = s.attention ?? ""
        return attention.isEmpty || attention == "input"
    }

    /// Mid-turn: no suggestion to ask for (Claude Code shows none), and the
    /// line says what you send waits.
    public static func busy(_ s: Session) -> Bool { s.state == .working }

    /// Why a click can't open a line, said quietly under the row for a moment.
    public enum Blocked: Equatable, Sendable {
        case notInTmux
        case dropped
    }

    public static func blocked(_ s: Session) -> Blocked? {
        guard !s.isMoss else { return nil }
        if s.state == .interrupted { return .dropped }
        if !s.inTmux && s.state != .closed { return .notInTmux }
        return nil
    }

    /// Waiting for you to type: the ones a run of replies goes through.
    public static func waiting(_ s: Session) -> Bool {
        opens(s) && !busy(s) && s.attention == "input"
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
