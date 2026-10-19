import Foundation

// Claude Code's /usage and /status screens, read back from the terminal
// text Moss returns (T2.3 AMEND-3), so the Moss page can draw them instead
// of showing a wall of monospaced text. Both parsers are tolerant: every
// section is optional, and a screen they can't make sense of returns nil
// (the page then shows the raw terminal text).

/// Lines of a captured screen, with the frame Claude Code draws around a
/// settings screen removed (the rule, the tab row, "Esc to cancel").
enum ScreenLines {
    static func of(_ text: String) -> [String] {
        text.replacingOccurrences(of: "\r\n", with: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map { String($0).replacingOccurrences(of: "\u{00A0}", with: " ") }
            .map { line in
                var l = line
                while l.last?.isWhitespace == true { l.removeLast() }
                return l
            }
            .filter { !isChrome($0) }
    }

    static func isChrome(_ line: String) -> Bool {
        let t = line.trimmingCharacters(in: .whitespaces)
        if t.isEmpty { return false }
        if t.allSatisfy({ $0 == "─" || $0 == "━" || $0 == "-" }) && t.count > 8 { return true }
        if t.hasPrefix("Esc to ") { return true }
        // The settings tab row: "Settings  Status   Config   Usage   Stats".
        let words = Set(t.split(whereSeparator: \.isWhitespace).map(String.init))
        if words.isSuperset(of: ["Status", "Config", "Usage"]) && words.count <= 7 { return true }
        return false
    }

    static func indent(_ line: String) -> Int { line.prefix { $0 == " " }.count }

    /// "a   b  c" → ["a", "b", "c"]: columns are two or more spaces apart.
    static func columns(_ line: String) -> [String] {
        line.trimmingCharacters(in: .whitespaces)
            .components(separatedBy: "  ")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }

    static func firstMatch(_ pattern: String, in text: String) -> [String]? {
        guard let re = try? NSRegularExpression(pattern: pattern),
              let m = re.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) else { return nil }
        return (0..<m.numberOfRanges).map { i in
            Range(m.range(at: i), in: text).map { String(text[$0]) } ?? ""
        }
    }

    /// "Claude Max" (or Pro, Team, Enterprise) wherever the screen names a plan.
    static func plan(in text: String) -> String? {
        firstMatch(#"Claude (Max|Pro|Team|Enterprise)\b"#, in: text).map { "Claude \($0[1])" }
    }
}

/// A command's screen without the frame of a settings screen (the rule,
/// the tab row, "Esc to cancel") and the blank lines around it, for the
/// raw view.
public enum MossScreenText {
    public static func withoutChrome(_ text: String) -> String {
        var lines = ScreenLines.of(text)
        while lines.first?.trimmingCharacters(in: .whitespaces).isEmpty == true { lines.removeFirst() }
        while lines.last?.trimmingCharacters(in: .whitespaces).isEmpty == true { lines.removeLast() }
        // Collapse runs of blank lines to one.
        var out: [String] = []
        for line in lines {
            if line.trimmingCharacters(in: .whitespaces).isEmpty, out.last?.trimmingCharacters(in: .whitespaces).isEmpty == true { continue }
            out.append(line)
        }
        // The margin every line shares goes too.
        let margin = out.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.map(ScreenLines.indent).min() ?? 0
        return out.map { String($0.dropFirst(min(margin, ScreenLines.indent($0)))) }.joined(separator: "\n")
    }
}

/// What /usage shows.
public struct MossUsage: Equatable, Sendable {
    /// One limit: "Current session", 13% used, resets 1:49pm.
    public struct Limit: Equatable, Sendable {
        public var label: String
        public var percent: Int
        /// Claude Code's own words ("Oct 23 at 1am"), without the time zone.
        public var resets: String?
    }

    /// One model's share of the session ("Usage by model").
    public struct ModelUsage: Equatable, Sendable {
        public var model: String
        public var input: String
        public var output: String
        /// "$0.0003", when shown.
        public var cost: String?
    }

    /// The "Session" block: this Claude Code session's totals.
    public struct Session: Equatable, Sendable {
        public var cost: String?
        public var apiDuration: String?
        public var wallDuration: String?
        public var added: Int?
        public var removed: Int?
        /// From a single "Usage:" line (older Claude Code).
        public var input: String?
        public var output: String?
        /// From "Usage by model:" (newer Claude Code), one row per model.
        public var models: [ModelUsage] = []
        /// "Prompt cache (main): 4 requests · 89% of input tokens from cache · …", unwrapped.
        public var cache: String?

        /// How much of the input came from the prompt cache.
        public var cacheHit: Int? {
            cache.flatMap { ScreenLines.firstMatch(#"(\d{1,3})% of input tokens from cache"#, in: $0) }.flatMap { Int($0[1]) }
        }

        /// The cache line without the hit rate: "4 requests · no misses · warm (1h TTL, …)".
        public var cacheDetail: String? {
            guard let cache else { return nil }
            let parts = cache.components(separatedBy: " · ").filter { !$0.contains("% of input tokens from cache") }
            return parts.isEmpty ? nil : parts.joined(separator: " · ")
        }

        /// Nothing spent, nothing changed: not worth showing.
        public var isEmpty: Bool {
            let zeroCost = cost.map { ScreenLines.firstMatch(#"^\$?0(\.0+)?$"#, in: $0) != nil } ?? true
            let zeroTokens = ([input, output] + models.flatMap { [$0.input, $0.output] }).allSatisfy { $0 == nil || $0 == "0" }
            return zeroCost && (added ?? 0) == 0 && (removed ?? 0) == 0 && zeroTokens
        }
    }

    /// "98% of your usage came from subagent-heavy sessions", and its advice.
    public struct Insight: Equatable, Sendable {
        public var percent: Int
        /// The words after "of your usage": "came from subagent-heavy sessions".
        public var headline: String
        public var hint: String
    }

    public struct Share: Equatable, Sendable {
        public var name: String
        public var percent: Int
    }

    /// "Skills", "Subagents", "MCP servers": who used how much.
    public struct Table: Equatable, Sendable {
        public var title: String
        public var rows: [Share]
    }

    public struct Loop: Equatable, Sendable {
        public var name: String
        public var every: String?
        public var runs: String?
        public var tokens: String?
        public var perRun: String?
        public var lastRun: String?
    }

    public var plan: String?
    public var limits: [Limit] = []
    public var session: Session?
    /// The line under "What's contributing…" that says what period it covers.
    public var insightsPeriod: String?
    public var insights: [Insight] = []
    public var tables: [Table] = []
    public var loops: [Loop] = []

    /// Nil when nothing on the screen is recognised.
    public static func parse(_ text: String) -> MossUsage? {
        var u = MossUsage()
        u.plan = ScreenLines.plan(in: text)
        let lines = ScreenLines.of(text)
        let percentUsed = #"^[\s█▉▊▋▌▍▎▏▐░▒▓]*?(\d{1,3})% used$"#
        var i = 0
        func next(after k: Int) -> Int? {
            var j = k + 1
            while j < lines.count, lines[j].trimmingCharacters(in: .whitespaces).isEmpty { j += 1 }
            return j < lines.count ? j : nil
        }
        while i < lines.count {
            let line = lines[i]
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.isEmpty { i += 1; continue }

            // A limit: its label, then the bar and "NN% used", then "Resets …".
            if let j = next(after: i), let m = ScreenLines.firstMatch(percentUsed, in: lines[j]),
               ScreenLines.firstMatch(percentUsed, in: line) == nil {
                var limit = Limit(label: t, percent: min(100, Int(m[1]) ?? 0))
                var end = j
                if let k = next(after: j), let r = ScreenLines.firstMatch(#"^\s*Resets\s+(.+)$"#, in: lines[k]) {
                    limit.resets = Self.withoutZone(r[1])
                    end = k
                }
                u.limits.append(limit)
                i = end + 1
                continue
            }

            if t == "Session" {
                var s = Session()
                var j = i + 1
                while j < lines.count, lines[j].trimmingCharacters(in: .whitespaces).isEmpty { j += 1 }
                // The block runs to the next blank line.
                var lastKey = ""
                while j < lines.count, !lines[j].trimmingCharacters(in: .whitespaces).isEmpty {
                    let line = lines[j]
                    j += 1
                    if lastKey == "Usage by model",
                       let m = ScreenLines.firstMatch(#"^\s+(\S+):\s+(\S+) input, (\S+) output.*?(\(\$[0-9.]+\))?$"#, in: line) {
                        let cost = m[4].isEmpty ? nil : String(m[4].dropFirst().dropLast())
                        s.models.append(ModelUsage(model: m[1], input: m[2], output: m[3], cost: cost))
                        continue
                    }
                    guard let kv = ScreenLines.firstMatch(#"^\s*([A-Za-z][A-Za-z ()]*):\s*(.*)$"#, in: line) else {
                        // A wrapped value (the cache line runs onto a second line).
                        if lastKey == "Prompt cache (main)" || lastKey == "Prompt cache", let cache = s.cache {
                            s.cache = cache + " " + line.trimmingCharacters(in: .whitespaces)
                        }
                        continue
                    }
                    let value = kv[2].trimmingCharacters(in: .whitespaces)
                    lastKey = kv[1]
                    switch kv[1] {
                    case "Total cost": s.cost = value
                    case "Total duration (API)": s.apiDuration = value
                    case "Total duration (wall)": s.wallDuration = value
                    case "Total code changes":
                        s.added = ScreenLines.firstMatch(#"(\d+) lines? added"#, in: value).flatMap { Int($0[1]) }
                        s.removed = ScreenLines.firstMatch(#"(\d+) lines? removed"#, in: value).flatMap { Int($0[1]) }
                    case "Usage":
                        s.input = ScreenLines.firstMatch(#"(\S+) input"#, in: value)?[1]
                        s.output = ScreenLines.firstMatch(#"(\S+) output"#, in: value)?[1]
                    case "Prompt cache (main)", "Prompt cache":
                        s.cache = value
                    default: break
                    }
                }
                u.session = s
                i = j
                continue
            }

            if t.hasPrefix("What's contributing") {
                var j = i + 1
                var current: Insight?
                while j < lines.count {
                    let l = lines[j].trimmingCharacters(in: .whitespaces)
                    if Self.tableTitle(lines[j]) != nil || Self.isLoopsHeader(lines[j]) { break }
                    if let m = ScreenLines.firstMatch(#"^(\d{1,3})% of your usage (.+)$"#, in: l) {
                        if let current { u.insights.append(current) }
                        current = Insight(percent: Int(m[1]) ?? 0, headline: m[2], hint: "")
                    } else if !l.isEmpty, var c = current {
                        c.hint = c.hint.isEmpty ? l : c.hint + " " + l
                        current = c
                    } else if !l.isEmpty, l.hasPrefix("Last ") {
                        u.insightsPeriod = l
                    }
                    j += 1
                }
                if let current { u.insights.append(current) }
                i = j
                continue
            }

            if let title = Self.tableTitle(line) {
                var rows: [Share] = []
                var j = i + 1
                while j < lines.count, let m = ScreenLines.firstMatch(#"^\s*(\S.*?)\s{2,}(\d{1,3})%$"#, in: lines[j]) {
                    rows.append(Share(name: m[1], percent: Int(m[2]) ?? 0))
                    j += 1
                }
                if !rows.isEmpty { u.tables.append(Table(title: title, rows: rows.sorted { $0.percent > $1.percent })) }
                i = j
                continue
            }

            if Self.isLoopsHeader(line) {
                var j = i + 1
                while j < lines.count {
                    let cols = ScreenLines.columns(lines[j])
                    guard cols.count >= 2, !lines[j].trimmingCharacters(in: .whitespaces).hasPrefix("d to ") else { break }
                    func col(_ k: Int) -> String? { k < cols.count ? cols[k] : nil }
                    u.loops.append(Loop(name: cols[0], every: col(1), runs: col(2), tokens: col(3), perRun: col(4),
                                        lastRun: col(5)))
                    j += 1
                }
                i = j
                continue
            }
            i += 1
        }
        guard !u.limits.isEmpty || u.session != nil || !u.tables.isEmpty || !u.insights.isEmpty else { return nil }
        return u
    }

    /// "Skills                  % of usage" → "Skills".
    static func tableTitle(_ line: String) -> String? {
        ScreenLines.firstMatch(#"^\s*(\S.*?)\s{2,}% of usage$"#, in: line)?[1]
    }

    static func isLoopsHeader(_ line: String) -> Bool {
        let t = line.trimmingCharacters(in: .whitespaces)
        return t.hasPrefix("Loops") && t.contains("every")
    }

    /// "1:49pm (Asia/Shanghai)" → "1:49pm".
    static func withoutZone(_ text: String) -> String {
        let t = text.trimmingCharacters(in: .whitespaces)
        if let m = ScreenLines.firstMatch(#"^(.*?)\s*\([A-Za-z_]+(/[A-Za-z_+\-]+)+\)$"#, in: t) { return m[1] }
        return t
    }
}

extension MossUsage.Limit {
    /// "Current week (all models)" → "本周（全部模型）"; unknown labels as written.
    public func title(_ lang: Lang = L.lang) -> String {
        if label == "Current session" {
            return lang == .zh ? "本次会话（5 小时）" : "Current session (5 hours)"
        }
        if let m = ScreenLines.firstMatch(#"^Current week \((.+)\)$"#, in: label) {
            let what = m[1] == "all models" ? (lang == .zh ? "全部模型" : "all models") : m[1]
            return lang == .zh ? "本周（\(what)）" : "This week (\(what))"
        }
        return label
    }

    /// How close it is: calm, getting there, nearly out.
    public enum Level: Equatable, Sendable { case calm, warm, hot }

    public var level: Level {
        switch percent {
        case ..<50: .calm
        case 50..<80: .warm
        default: .hot
        }
    }
}

extension MossUsage.Insight {
    /// The headline in plain words: a fixed set of Claude Code's phrases
    /// translated, anything else as written.
    public func title(_ lang: Lang = L.lang) -> String {
        let h = headline
        if lang == .zh {
            if h == "came from subagent-heavy sessions" { return "来自大量用子代理的会话" }
            if let m = ScreenLines.firstMatch(#"^came from sessions active for (\S+) hours$"#, in: h) {
                return "来自连续活跃 \(m[1].replacingOccurrences(of: "+", with: "")) 小时以上的会话"
            }
            if let m = ScreenLines.firstMatch(#"^was at >(\S+) context$"#, in: h) { return "发生在上下文超过 \(m[1]) 时" }
            if let m = ScreenLines.firstMatch(#"^was while (\d+)\+ sessions ran in parallel$"#, in: h) {
                return "发生在 \(m[1]) 个以上会话同时运行时"
            }
            if let m = ScreenLines.firstMatch(#"^came from subagents under "(.+)"$"#, in: h) { return "来自「\(m[1])」子代理" }
            return h
        }
        return h.prefix(1).uppercased() + h.dropFirst()
    }
}

extension MossUsage.Table {
    public func title(_ lang: Lang = L.lang) -> String {
        guard lang == .zh else { return title }
        return switch title {
        case "Skills": "技能"
        case "Subagents": "子代理"
        case "MCP servers": "MCP 服务"
        default: title
        }
    }
}

/// What /status shows: "Key: value" lines, long values wrapped onto
/// indented lines underneath.
public struct MossStatusScreen: Equatable, Sendable {
    public struct Field: Equatable, Sendable {
        public var key: String
        public var value: String
    }

    /// In screen order, wrapped values joined, "Peer address" dropped.
    public var fields: [Field]

    public func value(_ key: String) -> String? {
        fields.first { $0.key == key }.flatMap { $0.value.isEmpty ? nil : $0.value }
    }

    public var version: String? { value("Version") }
    /// "Default (Opus 5.5 · Best for everyday, complex tasks)" → the
    /// model and the note, split at " · " (or a wide gap).
    private var modelParts: (name: String, note: String?)? {
        guard let v = value("Model") else { return nil }
        guard let m = ScreenLines.firstMatch(#"\((.+)\)$"#, in: v) else { return (v, nil) }
        let inner = m[1]
        for separator in [" · ", "   ", "  "] {
            if let r = inner.range(of: separator) {
                return (String(inner[..<r.lowerBound]).trimmingCharacters(in: .whitespaces),
                        String(inner[r.upperBound...]).trimmingCharacters(in: .whitespaces))
            }
        }
        return (inner, nil)
    }

    /// "Opus 5.5".
    public var model: String? { modelParts?.name }

    /// "Best for everyday, complex tasks".
    public var modelNote: String? { modelParts?.note }

    /// "Claude Max" from "Claude Max account".
    public var plan: String? {
        guard let v = value("Login method") else { return nil }
        return v.hasSuffix(" account") ? String(v.dropLast(" account".count)) : v
    }

    /// Nil while the session has no name ("/rename to add a name").
    public var sessionName: String? {
        guard let v = value("Session name"), !v.hasPrefix("/rename") else { return nil }
        return v
    }

    /// IDE: connected or not, and the words without the mark.
    public var ide: (connected: Bool, text: String)? {
        guard let v = value("IDE") else { return nil }
        let connected = v.hasPrefix("✔") || v.hasPrefix("✓")
        let text = v.drop { "✔✓✘✗ ".contains($0) }
        return (connected, String(text))
    }

    /// "1 disabled   /mcp" → "1 disabled".
    public var mcp: String? {
        value("MCP servers").map { v in
            ScreenLines.columns(v.replacingOccurrences(of: " · ", with: "  "))
                .filter { !$0.hasPrefix("/") }
                .joined(separator: " · ")
        }
    }

    /// Keys the card shows in its own places.
    public static let known: Set<String> = ["Version", "Model", "Login method", "Organization", "Email", "Session name",
                                            "Session ID", "Session kind", "cwd", "IDE", "MCP servers", "Cloud sessions"]

    /// The rest, for an "Other" group; empty values left out.
    public var others: [Field] {
        fields.filter { !Self.known.contains($0.key) && !$0.value.isEmpty }
    }

    public static func parse(_ text: String) -> MossStatusScreen? {
        var fields: [Field] = []
        var valueColumn = 0
        for line in ScreenLines.of(text) {
            if line.trimmingCharacters(in: .whitespaces).isEmpty { continue }
            if let m = ScreenLines.firstMatch(#"^(\s{0,4})([A-Za-z][A-Za-z ()]{0,30}?):(\s*)(.*)$"#, in: line),
               ScreenLines.indent(line) < max(valueColumn, 6) {
                valueColumn = m[1].count + m[2].count + 1 + m[3].count
                fields.append(Field(key: m[2], value: m[4].trimmingCharacters(in: .whitespaces)))
            } else if var last = fields.popLast(), valueColumn > 0, ScreenLines.indent(line) >= valueColumn - 1 {
                // A wrapped value: a path breaks anywhere, words at a space.
                let more = line.trimmingCharacters(in: .whitespaces)
                last.value = last.value.contains(" ") ? last.value + " " + more : last.value + more
                fields.append(last)
            }
        }
        fields.removeAll { $0.key == "Peer address" }
        let recognised = fields.filter { known.contains($0.key) }.count
        guard recognised >= 3 else { return nil }
        return MossStatusScreen(fields: fields)
    }
}
