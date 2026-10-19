import Foundation

/// Searching the Sessions page (T2.3 AMEND-4): one ranked list while you
/// type. Name first (a prefix, then a word start or initials, pinyin for
/// Chinese names, then anywhere in the name), then the folder, the note, the
/// last thing said. Ties go to the most recently active.
public struct SearchHit: Equatable, Identifiable, Sendable {
    public var session: Session
    /// 0 is best.
    public var rank: Int
    /// Characters of the name (by offset) that matched, to highlight.
    public var nameRanges: [Range<Int>]
    /// Why it matched, when the name doesn't show it.
    public var reason: Reason?

    public var id: String { session.id }

    public enum Reason: Equatable, Sendable {
        /// The folder's path holds the word.
        case path(String)
        /// The note does: a short piece of it.
        case note(String)
        /// The last thing said does.
        case prompt(String)
    }
}

/// The ranks, best first.
public enum SearchRank {
    public static let namePrefix = 0
    public static let wordStart = 1
    public static let nameContains = 2
    public static let folder = 3
    public static let note = 4
    public static let prompt = 5
}

/// What a search looks at for one session, worked out once per refresh
/// (pinyin isn't free), not per keystroke.
public struct SearchIndex: Sendable {
    struct Word: Sendable {
        /// Lowercased: a Latin word, or a Chinese character's pinyin.
        var text: String
        /// Where it starts in the name, by character.
        var start: Int
        /// How many characters of the name it covers.
        var length: Int
        var chinese: Bool
    }

    struct Entry: Sendable {
        var session: Session
        var title: String
        var lower: String
        var words: [Word]
        var hasChinese: Bool
        var folder: String
        var path: String
        var notes: [String]
        var prompt: String?
    }

    let entries: [Entry]

    /// `folders`: each session's folder heading, by session id.
    public init(_ sessions: [Session], folders: [String: String] = [:]) {
        entries = sessions.map { s in
            let words = Self.words(s.title)
            return Entry(session: s, title: s.title, lower: s.title.lowercased(), words: words,
                         hasChinese: words.contains(where: \.chinese),
                         folder: (folders[s.id] ?? (s.cwd as NSString).lastPathComponent).lowercased(),
                         path: s.cwd.lowercased(),
                         notes: [s.notes.note, s.notes.state, s.notes.goal].compactMap { $0 }.filter { !$0.isEmpty },
                         prompt: s.lastPrompt)
        }
    }

    /// Words of a name: split at - _ . / and spaces and where lower case
    /// turns upper; each Chinese character is a word of its own, read as pinyin.
    static func words(_ title: String) -> [Word] {
        var out: [Word] = []
        var current = ""
        var start = 0
        var previous: Character?
        func flush(at i: Int) {
            if !current.isEmpty { out.append(Word(text: current.lowercased(), start: start, length: i - start, chinese: false)) }
            current = ""
        }
        for (i, ch) in title.enumerated() {
            if let py = pinyin(ch) {
                flush(at: i)
                out.append(Word(text: py, start: i, length: 1, chinese: true))
                previous = ch
                continue
            }
            let separator = ch == "-" || ch == "_" || ch == "." || ch == "/" || ch.isWhitespace
            if separator {
                flush(at: i)
            } else {
                let caseTurn = ch.isUppercase && (previous?.isLowercase ?? false)
                if caseTurn { flush(at: i) }
                if current.isEmpty { start = i }
                current.append(ch)
            }
            previous = ch
        }
        flush(at: title.count)
        return out
    }

    /// A Chinese character's pinyin, plain letters ("接" → "jie"); nil for anything else.
    static func pinyin(_ ch: Character) -> String? {
        guard let scalar = ch.unicodeScalars.first, ch.unicodeScalars.count == 1,
              (0x4E00...0x9FFF).contains(scalar.value) || (0x3400...0x4DBF).contains(scalar.value) else { return nil }
        let s = NSMutableString(string: String(ch))
        CFStringTransform(s, nil, kCFStringTransformMandarinLatin, false)
        CFStringTransform(s, nil, kCFStringTransformStripDiacritics, false)
        let out = (s as String).lowercased().filter(\.isLetter)
        return out.isEmpty ? nil : out
    }

    /// Ranked hits for `query` (every word of it has to match). Empty query: none.
    public func search(_ query: String) -> [SearchHit] {
        let terms = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !terms.isEmpty else { return [] }
        var hits: [SearchHit] = []
        for e in entries {
            var worst = 0
            var ranges: [Range<Int>] = []
            var reason: SearchHit.Reason?
            var all = true
            for term in terms {
                guard let m = Self.match(term, e) else { all = false; break }
                worst = max(worst, m.rank)
                ranges += m.ranges
                if reason == nil { reason = m.reason }
            }
            guard all else { continue }
            hits.append(SearchHit(session: e.session, rank: worst, nameRanges: Self.merged(ranges),
                                  reason: worst >= SearchRank.folder ? reason : nil))
        }
        return hits.sorted { a, b in
            a.rank != b.rank ? a.rank < b.rank : a.session.lastActiveAt > b.session.lastActiveAt
        }
    }

    static func match(_ q: String, _ e: Entry) -> (rank: Int, ranges: [Range<Int>], reason: SearchHit.Reason?)? {
        if e.lower.hasPrefix(q) { return (SearchRank.namePrefix, [0..<min(q.count, e.title.count)], nil) }
        if let r = wordStart(q, e) { return (SearchRank.wordStart, r, nil) }
        if let range = e.lower.range(of: q) {
            let start = e.lower.distance(from: e.lower.startIndex, to: range.lowerBound)
            return (SearchRank.nameContains, [start..<(start + q.count)], nil)
        }
        if e.folder.contains(q) { return (SearchRank.folder, [], nil) }
        if e.path.contains(q) { return (SearchRank.folder, [], .path(q)) }
        if let note = e.notes.first(where: { $0.lowercased().contains(q) }) { return (SearchRank.note, [], .note(snippet(note, q))) }
        if let p = e.prompt, p.lowercased().contains(q) { return (SearchRank.prompt, [], .prompt(snippet(p, q))) }
        return nil
    }

    /// A word that starts with it, the words' initials ("mds" → mac-dev-setup,
    /// "jkps" → 接口评审), or the pinyin from the start ("jiekou").
    static func wordStart(_ q: String, _ e: Entry) -> [Range<Int>]? {
        if let w = e.words.first(where: { !$0.chinese && $0.text.hasPrefix(q) }) {
            return [w.start..<(w.start + min(q.count, w.length))]
        }
        let initials = String(e.words.compactMap(\.text.first))
        if q.count >= 2, initials.hasPrefix(q) {
            return e.words.prefix(q.count).map { $0.start..<($0.start + 1) }
        }
        if e.hasChinese {
            var covered: [Range<Int>] = []
            var rest = Substring(q)
            for w in e.words {
                guard !rest.isEmpty else { break }
                if rest.hasPrefix(w.text) {
                    rest = rest.dropFirst(w.text.count)
                    covered.append(w.start..<(w.start + w.length))
                } else if w.text.hasPrefix(rest) {
                    rest = ""
                    covered.append(w.start..<(w.start + (w.chinese ? 1 : min(w.length, q.count))))
                } else {
                    return nil
                }
            }
            return rest.isEmpty && !covered.isEmpty ? covered : nil
        }
        return nil
    }

    /// About 30 characters around the match.
    static func snippet(_ text: String, _ q: String) -> String {
        let flat = text.split(whereSeparator: \.isNewline).joined(separator: " ")
        guard let r = flat.lowercased().range(of: q) else { return String(flat.prefix(30)) }
        let at = flat.lowercased().distance(from: flat.lowercased().startIndex, to: r.lowerBound)
        let chars = Array(flat)
        var from = max(0, at - 10)
        // Start at a word, not in the middle of one.
        if from > 0, let space = chars[from..<at].firstIndex(where: \.isWhitespace) { from = space + 1 }
        let end = min(chars.count, from + 30)
        return (from > 0 ? "…" : "") + String(chars[from..<end]) + (end < chars.count ? "…" : "")
    }

    static func merged(_ ranges: [Range<Int>]) -> [Range<Int>] {
        var out: [Range<Int>] = []
        for r in ranges.sorted(by: { $0.lowerBound < $1.lowerBound }) {
            if let last = out.last, r.lowerBound <= last.upperBound {
                out[out.count - 1] = last.lowerBound..<max(last.upperBound, r.upperBound)
            } else {
                out.append(r)
            }
        }
        return out
    }
}

extension MossClient {
    /// `cc-moss ls --json --all`: closed sessions too (searched lazily).
    public func snapshotAll() async throws -> Snapshot {
        let out = try await plainData(["ls", "--json", "--all"])
        do {
            return try Snapshot.decode(out)
        } catch SnapshotError.newerSchema(let n) {
            throw Failure.newerSchema(n)
        }
    }
}
