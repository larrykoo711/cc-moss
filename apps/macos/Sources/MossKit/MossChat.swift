import Foundation

// Talking to Moss from the panel (T2.3). The app never touches Moss's
// terminal: `cc-moss moss say` drops a message in Moss's inbox, the daemon
// hands it over and writes Moss's answer to the conversation log, and
// `cc-moss moss chat --json` reads that log back.

/// What Moss is doing, as `moss status --json` says (`activity`).
public enum MossActivity: String, CaseIterable, Equatable, Sendable {
    case idle
    /// In the middle of a turn.
    case thinking
    /// Stuck on a confirm prompt in its terminal.
    case waiting
    /// Switched on but not running (restarting, or the daemon is down).
    case offline
    /// Switched off.
    case off

    /// For an older cc-moss that does not say.
    static func derived(enabled: Bool, running: Bool, daemonRunning: Bool) -> MossActivity {
        guard enabled else { return .off }
        return running && daemonRunning ? .idle : .offline
    }

    /// Moss can take a message: it is on and running (busy is fine, the
    /// message waits its turn).
    public var canTalk: Bool { self != .off && self != .offline }
}

extension MossStatus {
    /// The Moss page's one line: "Standing by · up 2 hours", "Thinking…",
    /// or why it can't talk right now.
    public func activityLine(now: Date = L.clock(), lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        // Stuck text comes first: nothing else gets through until it's cleared.
        if inputBusy, activity.canTalk {
            return t("There's unsent text in Moss's input; take a look in the terminal",
                     "Moss 的输入框里有没送出去的字，去终端看看")
        }
        let line: String = switch activity {
        case .idle:
            if let since {
                t("Standing by · up \(DaemonStatus.span(from: since, to: now, lang: lang))",
                  "待命中 · 已运行 \(DaemonStatus.span(from: since, to: now, lang: lang))")
            } else {
                t("Standing by", "待命中")
            }
        case .thinking: t("Thinking…", "正在想…")
        case .waiting: t("Waiting for your OK, check the terminal", "在等你确认，去终端看看")
        case .offline: offlineReason(lang: lang)
        case .off: t("Off · turn it on in Settings", "已关闭 · 去设置里打开")
        }
        guard queued > 0, activity.canTalk else { return line }
        return line + t(" · \(queued) in line", " · 还排着 \(queued) 条")
    }

    /// Why Moss can't talk while it is on: the daemon is down, the last
    /// start failed, or it is on its way back.
    func offlineReason(lang: Lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        if !daemonRunning { return t("Not running in the background · start it in Settings", "后台没在运行 · 去设置里启动") }
        if let lastError { return t("Couldn't start: \(lastError)", "没能启动：\(lastError)") }
        return t("Restarting, try again in 30 seconds", "重启中，30 秒后再试")
    }

    /// Under the input when it's disabled: why you can't send right now.
    public func inputBlocked(lang: Lang = L.lang) -> String? {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        return switch activity {
        case .off: t("Moss is off. Turn it on in Settings to talk to it.", "Moss 关着，去设置里打开就能聊。")
        case .offline: offlineReason(lang: lang)
        default: nil
        }
    }
}

/// One line of the conversation.
public struct MossMessage: Identifiable, Equatable, Sendable {
    public enum Sender: Equatable, Sendable {
        case you
        case moss
        /// A digest of what the sessions did, shown as a quiet line.
        case events
        /// A sender a newer cc-moss knows (Slack, later): shown like yours.
        case other(String)

        init(_ raw: String) {
            switch raw {
            case "you": self = .you
            case "moss": self = .moss
            case "events": self = .events
            default: self = .other(raw)
            }
        }
    }

    /// How far a message you sent has got.
    public enum Delivery: String, Equatable, Sendable {
        /// Still on its way to cc-moss (the app only).
        case sending
        /// In Moss's inbox, waiting its turn.
        case queued
        case sent
        case failed
        case replied
    }

    public var id: String
    public var at: Date?
    public var from: Sender
    public var text: String
    public var delivery: Delivery?
    public var error: String?
    public var replyTo: String?
    /// Moss got it, but its answer couldn't be read back.
    public var fallback: Bool
    /// Moss's suggested replies; clicking one sends it.
    public var buttons: [String]
    /// A Claude Code command Moss ran for you (`/usage`); `text` is then its
    /// screen as plain terminal text, not Markdown.
    public var command: String?
    /// Moss wouldn't run that command here (it needs the terminal).
    public var refused: Bool
    /// Moss's picks for the quick bar (`[[quick: /usage | waiting]]`):
    /// slash commands or action ids.
    public var quick: [String]

    public init(id: String, at: Date? = nil, from: Sender, text: String, delivery: Delivery? = nil, error: String? = nil,
                replyTo: String? = nil, fallback: Bool = false, buttons: [String] = [], command: String? = nil,
                refused: Bool = false, quick: [String] = []) {
        self.id = id
        self.at = at
        self.from = from
        self.text = text
        self.delivery = delivery
        self.error = error
        self.replyTo = replyTo
        self.fallback = fallback
        self.buttons = buttons
        self.command = command
        self.refused = refused
        self.quick = quick
    }

    /// What the bubble says: a fallback reads as a receipt.
    public func shownText(_ lang: Lang = L.lang) -> String {
        guard fallback else { return text }
        return lang == .zh ? "Moss 收到了（没能取回回复，去终端看）"
                           : "Moss got it (couldn't fetch the reply, check the terminal)"
    }
}

/// The answer of `cc-moss moss chat --json` (schema 1), oldest first.
public struct MossChat: Equatable, Sendable {
    public var messages: [MossMessage]
    /// Moss's newest quick-bar pin (`"quick":{"items":[…],"at":…}`), even
    /// from a reply that wasn't shown; nil from an older cc-moss.
    public var pin: Pin?

    /// What Moss pinned to the quick bar, and when.
    public struct Pin: Equatable, Sendable {
        public var items: [String]
        public var at: Date?

        public init(items: [String], at: Date?) {
            self.items = items
            self.at = at
        }
    }

    public init(messages: [MossMessage] = [], pin: Pin? = nil) {
        self.messages = messages
        self.pin = pin
    }

    private struct Wire: Decodable {
        var schema: Int
        var messages: [Line]?
        var quick: WirePin?
    }

    private struct WirePin: Decodable {
        var items: [String]?
        var at: String?
    }

    private struct Line: Decodable {
        var id: String
        var at: String?
        var from: String
        var text: String?
        var state: String?
        var error: String?
        var reply_to: String?
        var fallback: Bool?
        var buttons: [String]?
        var command: String?
        var refused: Bool?
        var quick: [String]?
    }

    public static func decode(_ data: Data) throws -> MossChat {
        let wire: Wire
        do {
            wire = try JSONDecoder().decode(Wire.self, from: data)
        } catch {
            throw MossClient.Failure.unreadable("\(error)")
        }
        guard wire.schema <= 1 else { throw MossClient.Failure.newerSchema(wire.schema) }
        let pin = wire.quick.map { p in
            Pin(items: (p.items ?? []).filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }, at: p.at.flatMap(parseTimestamp))
        }
        return MossChat(pin: pin, messages: (wire.messages ?? []).map { line in
            MossMessage(id: line.id, at: line.at.flatMap(parseTimestamp), from: .init(line.from), text: line.text ?? "",
                        delivery: line.state.flatMap(MossMessage.Delivery.init(rawValue:)),
                        error: line.error.flatMap { $0.isEmpty ? nil : $0 },
                        replyTo: line.reply_to, fallback: line.fallback ?? false,
                        buttons: (line.buttons ?? []).filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty },
                        command: line.command.flatMap { $0.isEmpty ? nil : $0 }, refused: line.refused ?? false,
                        quick: (line.quick ?? []).filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty })
        })
    }

    private init(pin: Pin?, messages: [MossMessage]) {
        self.messages = messages
        self.pin = pin
    }
}

/// Something Moss can do for you, offered as a button on the Moss page.
/// Data, not views: later abilities (a check-up, handing out work) are one
/// more entry in `all`.
public struct MossAction: Identifiable, Equatable, Sendable {
    public var id: String
    public var symbol: String
    var titles: (en: String, zh: String)
    var details: (en: String, zh: String)
    var texts: (en: String, zh: String)

    public static func == (a: MossAction, b: MossAction) -> Bool { a.id == b.id }

    public func title(_ lang: Lang = L.lang) -> String { lang == .zh ? titles.zh : titles.en }
    /// One line on what it covers, for the page's intro.
    public func detail(_ lang: Lang = L.lang) -> String { lang == .zh ? details.zh : details.en }
    /// What is sent to Moss, word for word.
    public func text(_ lang: Lang = L.lang) -> String { lang == .zh ? texts.zh : texts.en }

    /// What Moss can do today: it reads the session list, nothing else.
    public static let all: [MossAction] = [
        MossAction(id: "needs-you", symbol: "hand.raised",
                   titles: ("Who's waiting on me", "谁在等我"),
                   details: ("Sessions with a question or waiting for your reply", "有问题要答、等你回话的会话"),
                   texts: ("Which sessions are waiting on me? Most urgent first.", "现在哪些会话在等我？按急的先说。")),
        MossAction(id: "stuck", symbol: "exclamationmark.triangle",
                   titles: ("What's stuck", "谁卡住了"),
                   details: ("Ones that dropped or stopped moving", "断了或者停着不动的"),
                   texts: ("Which sessions dropped or got stuck, and where?", "有哪些会话断了或者卡住了？各自卡在哪？")),
        MossAction(id: "today", symbol: "checklist",
                   titles: ("What got done today", "今天做了什么"),
                   details: ("What each session finished", "各会话做完的事"),
                   texts: ("What did the sessions finish today? Keep it short.", "今天各个会话做完了什么？简短列一下。")),
    ]
}

/// One block of Moss's words.
public enum MossBlock: Equatable, Sendable {
    public struct Item: Equatable, Sendable {
        /// Nesting, 0 at the left.
        public var level: Int
        /// "3." for a numbered item, nil for a bullet.
        public var marker: String?
        public var text: String
        public init(level: Int, marker: String?, text: String) {
            self.level = level
            self.marker = marker
            self.text = text
        }
    }

    case paragraph(String)
    case heading(Int, String)
    case list(ordered: Bool, items: [Item])
    case quote(String)
    case code(String)
    case table(header: [String], rows: [[String]])
    case rule
}

/// Moss's words as rich text: inline Markdown (bold, italics, `code`,
/// links), whitespace kept. Only https links stay clickable; anything else
/// (file://, a custom scheme) is plain text.
public enum MossMarkdown {
    public static func inline(_ raw: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        guard var out = try? AttributedString(markdown: raw, options: options) else { return AttributedString(raw) }
        for run in out.runs {
            if let url = run.link, url.scheme?.lowercased() != "https" {
                out[run.range].link = nil
            }
        }
        return out
    }

    /// Moss's words as blocks: paragraphs, headings, lists, quotes, rules,
    /// fenced code and pipe tables. Inline Markdown stays in the text, for
    /// `inline` to render.
    public static func blocks(_ text: String) -> [MossBlock] {
        var out: [MossBlock] = []
        var para: [String] = []
        var quote: [String] = []
        var items: [MossBlock.Item] = []
        var ordered = false
        var code: [String]?
        let lines = normalised(text).components(separatedBy: "\n")

        func flushPara() {
            if !para.isEmpty { out.append(.paragraph(para.joined(separator: "\n"))) }
            para = []
        }
        func flushQuote() {
            if !quote.isEmpty { out.append(.quote(quote.joined(separator: "\n"))) }
            quote = []
        }
        func flushList() {
            if !items.isEmpty { out.append(.list(ordered: ordered, items: items)) }
            items = []
        }
        func flushAll() { flushPara(); flushQuote(); flushList() }

        var i = 0
        while i < lines.count {
            let line = lines[i]
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            i += 1
            if var body = code {
                if trimmed.hasPrefix("```") {
                    out.append(.code(body.joined(separator: "\n")))
                    code = nil
                } else {
                    body.append(line)
                    code = body
                }
                continue
            }
            if trimmed.hasPrefix("```") { flushAll(); code = []; continue }
            if trimmed.isEmpty { flushAll(); continue }
            if let (level, title) = heading(trimmed) { flushAll(); out.append(.heading(level, title)); continue }
            if isRule(trimmed) { flushAll(); out.append(.rule); continue }
            if trimmed.hasPrefix(">") {
                flushPara(); flushList()
                quote.append(String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces))
                continue
            }
            if let item = listItem(line) {
                flushPara(); flushQuote()
                if items.isEmpty { ordered = item.marker != nil }
                items.append(item)
                continue
            }
            if trimmed.hasPrefix("|"), i < lines.count, isTableRule(lines[i]) {
                flushAll()
                let header = cells(trimmed)
                i += 1
                var rows: [[String]] = []
                while i < lines.count, lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("|") {
                    rows.append(cells(lines[i]))
                    i += 1
                }
                out.append(.table(header: header, rows: rows))
                continue
            }
            flushQuote(); flushList()
            para.append(line)
        }
        if let body = code { out.append(.code(body.joined(separator: "\n"))) }
        flushAll()
        return out
    }

    private static func heading(_ line: String) -> (Int, String)? {
        let hashes = line.prefix(while: { $0 == "#" }).count
        guard (1...6).contains(hashes), line.dropFirst(hashes).first == " " else { return nil }
        return (hashes, line.dropFirst(hashes).trimmingCharacters(in: .whitespaces))
    }

    private static func isRule(_ line: String) -> Bool {
        let chars = line.filter { $0 != " " }
        guard chars.count >= 3, let first = chars.first, "-*_".contains(first) else { return false }
        return chars.allSatisfy { $0 == first }
    }

    private static func listItem(_ line: String) -> MossBlock.Item? {
        let indent = line.prefix(while: { $0 == " " || $0 == "\t" })
            .reduce(0) { $0 + ($1 == "\t" ? 4 : 1) }
        let rest = line.drop(while: { $0 == " " || $0 == "\t" })
        let level = min(indent / 2, 3)
        if let first = rest.first, "-*+".contains(first), rest.dropFirst().first == " " {
            return .init(level: level, marker: nil, text: rest.dropFirst(2).trimmingCharacters(in: .whitespaces))
        }
        let digits = rest.prefix(while: \.isNumber)
        guard !digits.isEmpty, digits.count <= 3 else { return nil }
        let after = rest.dropFirst(digits.count)
        guard let mark = after.first, mark == "." || mark == ")", after.dropFirst().first == " " else { return nil }
        return .init(level: level, marker: digits + ".", text: after.dropFirst(2).trimmingCharacters(in: .whitespaces))
    }

    private static func isTableRule(_ line: String) -> Bool {
        let t = line.trimmingCharacters(in: .whitespaces)
        guard t.hasPrefix("|"), t.contains("-") else { return false }
        return t.allSatisfy { "|-: ".contains($0) }
    }

    private static func cells(_ line: String) -> [String] {
        var t = line.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("|") { t.removeFirst() }
        if t.hasSuffix("|") { t.removeLast() }
        return t.components(separatedBy: "|").map { $0.trimmingCharacters(in: .whitespaces) }
    }

    /// What `moss say` takes: line breaks as \n only (cc-moss refuses \r).
    public static func normalised(_ text: String) -> String {
        text.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n")
    }
}

extension MossClient {
    /// `cc-moss moss chat --json`: the conversation, oldest first.
    public func mossChat() async throws -> MossChat {
        try MossChat.decode(Data(try await plain(["moss", "chat", "--json"]).utf8))
    }

    /// `cc-moss moss say --json -- <text>`: hand Moss a message; its id.
    /// cc-moss says no (in plain words) when Moss is off or not running.
    public func mossSay(_ text: String) async throws -> String {
        struct Answer: Decodable {
            var schema: Int
            var id: String
        }
        let out = try await plain(["moss", "say", "--json", "--", text])
        do {
            return try JSONDecoder().decode(Answer.self, from: Data(out.utf8)).id
        } catch {
            throw MossClient.Failure.unreadable("\(error)")
        }
    }
}
