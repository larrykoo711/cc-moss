import Foundation

/// Sessions named in Moss's words, found so the app can make each name a
/// mention: click to go to the session, hover for its card (user 2026-10-19).
public enum MossMentions {
    /// A session's name and slot id. Earlier ones win when two sessions
    /// share a name: the reply's own sessions come first.
    public struct Name: Equatable, Sendable {
        public var name: String
        public var id: String

        public init(_ name: String, id: String) {
            self.name = name
            self.id = id
        }
    }

    /// Where a name sits in the text, by character offset.
    public struct Mention: Equatable, Sendable {
        public var range: Range<Int>
        public var id: String

        public init(range: Range<Int>, id: String) {
            self.range = range
            self.id = id
        }
    }

    /// The link scheme a mention carries in rendered text.
    public static let scheme = "ccmoss-mention"

    public static func url(_ id: String) -> URL? {
        var c = URLComponents()
        c.scheme = scheme
        c.host = "session"
        c.path = "/" + id
        return c.url
    }

    /// The slot id a mention link points at; nil for any other link.
    public static func id(of url: URL) -> String? {
        guard url.scheme == scheme else { return nil }
        let id = String(url.path.dropFirst())
        return id.isEmpty ? nil : id
    }

    /// Mentions of `names` in `text`: longest name first, no overlaps, whole
    /// words only (a Latin name isn't matched inside a longer word; a Chinese
    /// name only needs no Latin letters or digits glued to it), never inside
    /// `skip` (code, links) or a bare URL.
    public static func find(text: String, names: [Name], skip: [Range<Int>] = []) -> [Mention] {
        let chars = Array(text)
        var taken = [Bool](repeating: false, count: chars.count)
        for r in skip + urls(chars) {
            for i in r where i >= 0 && i < chars.count { taken[i] = true }
        }
        // One id per name (the first given), longest names first.
        var seen = Set<String>()
        let candidates = names
            .filter { usable($0.name) && seen.insert($0.name).inserted }
            .sorted { $0.name.count > $1.name.count }
        var out: [Mention] = []
        for candidate in candidates {
            let needle = Array(candidate.name)
            let chinese = needle.contains(where: isCJK)
            var i = 0
            while i + needle.count <= chars.count {
                let range = i..<(i + needle.count)
                if chars[range].elementsEqual(needle), !range.contains(where: { taken[$0] }),
                   bounded(chars, range, chinese: chinese) {
                    out.append(Mention(range: range, id: candidate.id))
                    for j in range { taken[j] = true }
                    i += needle.count
                } else {
                    i += 1
                }
            }
        }
        return out.sorted { $0.range.lowerBound < $1.range.lowerBound }
    }

    /// Rendered inline text with each mention made a `ccmoss-mention:` link.
    /// Code and existing links are left alone.
    public static func mark(_ text: AttributedString, names: [Name]) -> AttributedString {
        guard !names.isEmpty else { return text }
        var out = text
        let plain = String(out.characters)
        var skip: [Range<Int>] = []
        for run in out.runs where run.link != nil || run.inlinePresentationIntent?.contains(.code) == true {
            let start = out.characters.distance(from: out.startIndex, to: run.range.lowerBound)
            let end = out.characters.distance(from: out.startIndex, to: run.range.upperBound)
            skip.append(start..<end)
        }
        for m in find(text: plain, names: names, skip: skip) {
            guard let url = url(m.id) else { continue }
            let lo = out.characters.index(out.startIndex, offsetBy: m.range.lowerBound)
            let hi = out.characters.index(out.startIndex, offsetBy: m.range.upperBound)
            out[lo..<hi].link = url
        }
        return out
    }

    /// Short names would light up ordinary words: Latin ones need three
    /// characters, Chinese ones two.
    static func usable(_ name: String) -> Bool {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard trimmed == name, !name.isEmpty else { return false }
        return name.contains(where: isCJK) ? name.count >= 2 : name.count >= 3
    }

    /// A Latin name stops at anything that isn't part of a name; a Chinese
    /// name sits among Chinese words, so only Latin letters and digits
    /// glued to it count.
    static func bounded(_ chars: [Character], _ range: Range<Int>, chinese: Bool) -> Bool {
        func joins(_ i: Int) -> Bool {
            guard i >= 0, i < chars.count else { return false }
            let c = chars[i]
            if chinese { return c.isASCII && (c.isLetter || c.isNumber || c == "-" || c == "_") }
            return c.isLetter || c.isNumber || c == "-" || c == "_"
        }
        return !joins(range.lowerBound - 1) && !joins(range.upperBound)
    }

    static func isCJK(_ c: Character) -> Bool {
        guard let v = c.unicodeScalars.first?.value else { return false }
        return (0x4E00...0x9FFF).contains(v) || (0x3400...0x4DBF).contains(v)
    }

    /// Bare URLs in the text: a name inside one isn't a mention.
    static func urls(_ chars: [Character]) -> [Range<Int>] {
        var out: [Range<Int>] = []
        let text = String(chars)
        guard let regex = try? NSRegularExpression(pattern: #"[a-zA-Z][a-zA-Z0-9+.-]*://\S+"#) else { return [] }
        let ns = text as NSString
        for m in regex.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
            guard let r = Range(m.range, in: text) else { continue }
            let lo = text.distance(from: text.startIndex, to: r.lowerBound)
            out.append(lo..<(lo + text[r].count))
        }
        return out
    }
}

/// Hovering mentions and their card: which badge is lit, which card shows,
/// and what the app should start or cancel. A card follows the pointer
/// after a moment; another badge switches it at once; leaving the badge and
/// the card closes it after a short grace.
public struct MentionHoverMachine: Equatable, Sendable {
    public enum Effect: Equatable, Sendable {
        case none
        /// Cancel any closing on its way; nothing else changes.
        case keep
        /// Show this card after a moment (cancel any other timer).
        case showLater(String)
        /// Showing this card now (cancel any other timer).
        case showNow(String)
        /// Close the card after a short grace.
        case hideLater
    }

    public private(set) var lit: String?
    public private(set) var card: String?
    private var onCard = false

    public init() {}

    public mutating func entered(_ id: String) -> Effect {
        if lit == id && (card == id || card == nil) { return .keep }
        lit = id
        if card == id { return .keep }
        if card != nil {
            card = id
            return .showNow(id)
        }
        return .showLater(id)
    }

    /// Only the badge that is lit can be left: a late "left" from the
    /// previous one changes nothing.
    public mutating func left(_ id: String) -> Effect {
        guard lit == id else { return .none }
        lit = nil
        return .hideLater
    }

    public mutating func cardHovered(_ inside: Bool) -> Effect {
        onCard = inside
        if inside { return .keep }
        return lit == nil ? .hideLater : .none
    }

    /// The moment passed: show the card if the pointer is still on `id`.
    public mutating func showFired(_ id: String) -> Bool {
        guard lit == id else { return false }
        card = id
        return true
    }

    /// The grace passed: close, unless the pointer is back on a badge or on the card.
    public mutating func hideFired() {
        guard lit == nil, !onCard else { return }
        card = nil
    }

    /// The page or panel went, or a badge was clicked: nothing is hovered.
    public mutating func reset() {
        lit = nil
        card = nil
        onCard = false
    }
}
