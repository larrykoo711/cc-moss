import Foundation

/// A paragraph that names sessions, cut into pieces a flow layout can wrap:
/// words and Chinese characters of plain text (their inline styles kept),
/// each mention whole, and the paragraph's own line breaks.
public enum MentionFlow {
    public enum Piece: Equatable, Sendable {
        /// One wrap unit of plain text: a word with its trailing space, or a
        /// Chinese character with the punctuation that can't be split from it.
        case text(AttributedString)
        /// A session's name, never split.
        case mention(id: String, name: String)
        /// A line break in the paragraph itself.
        case lineBreak
    }

    /// The pieces of rendered inline text whose mentions carry
    /// `MossMentions` links.
    public static func pieces(_ text: AttributedString) -> [Piece] {
        var out: [Piece] = []
        for run in text.runs {
            let part = AttributedString(text[run.range])
            if let url = run.link, let id = MossMentions.id(of: url) {
                out.append(.mention(id: id, name: String(part.characters)))
                continue
            }
            let plain = String(part.characters)
            // Each line of the run, its units, then a break between lines.
            var start = 0
            for (i, line) in plain.split(separator: "\n", omittingEmptySubsequences: false).enumerated() {
                if i > 0 { out.append(.lineBreak) }
                let chars = line.count
                for unit in units(String(line)) {
                    let lo = part.characters.index(part.startIndex, offsetBy: start + unit.lowerBound)
                    let hi = part.characters.index(part.startIndex, offsetBy: start + unit.upperBound)
                    out.append(.text(AttributedString(part[lo..<hi])))
                }
                start += chars + 1
            }
        }
        return trimmed(out)
    }

    /// The badge keeps its own room on either side, so the spaces around a
    /// name go: a lone space next to a badge, and the trailing spaces of the
    /// word before one ("Both " before a badge reads "Both").
    static func trimmed(_ pieces: [Piece]) -> [Piece] {
        func isMention(_ i: Int) -> Bool {
            guard pieces.indices.contains(i), case .mention = pieces[i] else { return false }
            return true
        }
        return pieces.indices.compactMap { i in
            guard case let .text(t) = pieces[i] else { return pieces[i] }
            let plain = String(t.characters)
            if plain.allSatisfy({ $0 == " " }), isMention(i - 1) || isMention(i + 1) { return nil }
            guard isMention(i + 1), plain.last == " " else { return pieces[i] }
            let keep = plain.count - plain.reversed().prefix { $0 == " " }.count
            let end = t.characters.index(t.startIndex, offsetBy: keep)
            return .text(AttributedString(t[t.startIndex..<end]))
        }
    }

    /// For each piece: it may not start a line (it begins with closing
    /// punctuation, as "，" after a badge does), so it wraps together with
    /// the piece before it.
    public static func glued(_ pieces: [Piece]) -> [Bool] {
        pieces.enumerated().map { i, piece in
            guard i > 0, case let .text(t) = piece, let first = t.characters.first else { return false }
            if case .lineBreak = pieces[i - 1] { return false }
            return noBreakBefore.contains(first)
        }
    }

    /// Punctuation that never starts a line: it stays with what's before it.
    static let noBreakBefore: Set<Character> = Set("。，、！？：；）」』】〉》”’…,.!?:;)]}%")
    /// Punctuation that never ends a line: it goes with what follows.
    static let noBreakAfter: Set<Character> = Set("（「『【〈《“‘([{")

    /// Where a line may wrap, as character ranges: a Latin word with its
    /// trailing spaces is one unit, each Chinese character is one, and
    /// opening and closing punctuation keep to their neighbours.
    public static func units(_ text: String) -> [Range<Int>] {
        let chars = Array(text)
        var out: [Range<Int>] = []
        var i = 0
        while i < chars.count {
            var end = i
            if wide(chars[i]) || noBreakAfter.contains(chars[i]) {
                // Openers, then one character (or a word) they lead into.
                while end < chars.count, noBreakAfter.contains(chars[end]) { end += 1 }
                if end < chars.count {
                    if wide(chars[end]) { end += 1 } else { end = wordEnd(chars, from: end) }
                }
            } else {
                end = wordEnd(chars, from: i)
            }
            // Closers and spaces stay on this line.
            while end < chars.count, noBreakBefore.contains(chars[end]) || chars[end] == " " { end += 1 }
            // Full-width punctuation ending a unit is set half-width (it would
            // end a line): the Chinese character, or the word, after it comes along.
            if end > i, end < chars.count, wide(chars[end - 1]), noBreakBefore.contains(chars[end - 1]),
               chars[end] != " ", !noBreakAfter.contains(chars[end]), !noBreakBefore.contains(chars[end]) {
                end = wide(chars[end]) ? end + 1 : wordEnd(chars, from: end)
                while end < chars.count, chars[end] == " " { end += 1 }
            }
            out.append(i..<max(end, i + 1))
            i = max(end, i + 1)
        }
        return out
    }

    /// The end of the Latin word starting at `start` (not its spaces).
    private static func wordEnd(_ chars: [Character], from start: Int) -> Int {
        var end = start
        while end < chars.count, chars[end] != " ", !wide(chars[end]), !(end > start && noBreakAfter.contains(chars[end])) {
            end += 1
        }
        return end
    }

    /// Chinese, Japanese and Korean characters and full-width punctuation:
    /// a line may break between any two of them.
    static func wide(_ c: Character) -> Bool {
        guard let v = c.unicodeScalars.first?.value else { return false }
        return (0x2E80...0x9FFF).contains(v) || (0xAC00...0xD7AF).contains(v)
            || (0xF900...0xFAFF).contains(v) || (0xFF00...0xFFEF).contains(v) || (0x3000...0x303F).contains(v)
    }
}
