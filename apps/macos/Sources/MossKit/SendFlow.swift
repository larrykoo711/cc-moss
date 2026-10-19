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

/// The pinned Moss row's second line: the start of Moss's newest reply, in
/// plain words. Never the last thing typed into Moss's terminal (the
/// daemon's "[Update] …" digests, a slash command).
public enum MossRowLine {
    public static func of(_ messages: [MossMessage]) -> String? {
        for m in messages.reversed() where m.from == .moss && m.command == nil && !m.fallback {
            if let line = firstLine(m.text) { return line }
        }
        return nil
    }

    static func firstLine(_ markdown: String) -> String? {
        for raw in markdown.split(whereSeparator: \.isNewline) {
            let line = raw.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty, !line.hasPrefix("```") else { continue }
            let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
            var plain = (try? AttributedString(markdown: line, options: options)).map { String($0.characters) } ?? line
            // Drop list and heading marks the inline parser keeps.
            while let first = plain.first, "#>-*·•".contains(first) { plain = String(plain.dropFirst()).trimmingCharacters(in: .whitespaces) }
            plain = plain.trimmingCharacters(in: .whitespaces)
            if plain.isEmpty || plain.hasPrefix("/") || plain.hasPrefix("[") { continue }
            return plain
        }
        return nil
    }
}
