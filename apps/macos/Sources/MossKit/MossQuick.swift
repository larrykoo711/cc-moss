import Foundation

// The Moss page's quick bar (T2.3 AMEND-3): Moss's abilities and the
// Claude Code commands that fit the account, in one row, the ones you use
// most first. What doesn't fit goes behind "⋯".

/// One thing the quick bar can send.
public struct MossQuickItem: Identifiable, Equatable, Sendable {
    /// Stable across launches: "action:needs-you", "cmd:/usage".
    public var id: String
    public var symbol: String
    var titles: (en: String, zh: String)
    var texts: (en: String, zh: String)
    /// The slash command, for a command item.
    public var command: String?

    public static func == (a: MossQuickItem, b: MossQuickItem) -> Bool { a.id == b.id }

    public func title(_ lang: Lang = L.lang) -> String { lang == .zh ? titles.zh : titles.en }
    /// What is sent to Moss.
    public func text(_ lang: Lang = L.lang) -> String { lang == .zh ? texts.zh : texts.en }

    public init(action: MossAction) {
        id = "action:\(action.id)"
        symbol = action.symbol
        titles = (action.title(.en), action.title(.zh))
        texts = (action.text(.en), action.text(.zh))
        command = nil
    }

    init(command: MossCommand, symbol: String, titles: (en: String, zh: String)) {
        id = "cmd:\(command.name)"
        self.symbol = symbol
        self.titles = titles
        texts = (command.name, command.name)
        self.command = command.name
    }
}

/// When an item was used, for the ranking.
public struct MossQuickUse: Codable, Equatable, Sendable {
    public var id: String
    public var at: Date

    public init(id: String, at: Date) {
        self.id = id
        self.at = at
    }
}

public enum MossQuick {
    /// Friendly names and symbols for the commands, in `MossCommand` order.
    static let commandLooks: [String: (symbol: String, en: String, zh: String)] = [
        "/usage": ("gauge.with.dots.needle.50percent", "Usage", "用量"),
        "/status": ("person.crop.circle", "Account", "账号"),
        "/context": ("square.stack.3d.up", "Context", "上下文"),
        "/cost": ("dollarsign.circle", "Cost", "花费"),
        "/stats": ("chart.bar", "Stats", "统计"),
    ]

    /// Everything, in the default order: the actions, then the commands
    /// that fit the account.
    public static func items(account: MossAccount = .unknown) -> [MossQuickItem] {
        let actions = MossAction.all.map(MossQuickItem.init(action:))
        let commands = MossCommand.all.filter { $0.fits(account) }.map { c in
            let look = commandLooks[c.name] ?? ("terminal", c.name, c.name)
            return MossQuickItem(command: c, symbol: look.symbol, titles: (look.en, look.zh))
        }
        return actions + commands
    }

    /// The item a sent text is, if any: an action's words (either language)
    /// or a command name.
    public static func item(sent text: String, account: MossAccount = .unknown) -> MossQuickItem? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return items(account: .unknown).first { item in
            if let command = item.command { return t.lowercased() == command }
            return t == item.text(.en) || t == item.text(.zh)
        }
    }

    /// Moss's word for an item in `[[quick: …]]`: a slash command or an
    /// action id ("waiting" is taken for "needs-you").
    public static func id(fromMoss token: String) -> String? {
        let t = token.trimmingCharacters(in: .whitespaces).lowercased()
        if t.hasPrefix("/") { return MossCommand.isAllowed(t) ? "cmd:\(t)" : nil }
        let action = t == "waiting" ? "needs-you" : t
        return MossAction.all.contains { $0.id == action } ? "action:\(action)" : nil
    }

    /// What's going on right now, beyond what you used before.
    public struct Context: Equatable, Sendable {
        /// The To Do page has something.
        public var queueHasItems = false
        /// A session dropped.
        public var anyInterrupted = false
        /// The last /usage showed a limit at 80% or more.
        public var usageHot = false
        /// Moss's latest picks (item ids, in its order), when younger than a day.
        public var mossPicks: [String] = []

        public init(queueHasItems: Bool = false, anyInterrupted: Bool = false, usageHot: Bool = false,
                    mossPicks: [String] = []) {
            self.queueHasItems = queueHasItems
            self.anyInterrupted = anyInterrupted
            self.usageHot = usageHot
            self.mossPicks = mossPicks
        }
    }

    public static let halfLifeDays: Double = 7
    public static let boost: Double = 3

    /// How much `id` was used: each use counts 1, halving every week.
    public static func score(_ id: String, uses: [MossQuickUse], now: Date) -> Double {
        uses.filter { $0.id == id }.reduce(0) { sum, use in
            let days = max(0, now.timeIntervalSince(use.at) / 86_400)
            return sum + pow(0.5, days / halfLifeDays)
        }
    }

    /// The bar's order: Moss's picks first (in its order), then by use and
    /// the moment's boosts; ties keep the default order.
    public static func ranked(_ items: [MossQuickItem], uses: [MossQuickUse], context: Context, now: Date) -> [MossQuickItem] {
        let bar = items
        let picks = context.mossPicks.compactMap { id in bar.first { $0.id == id } }
        let pickedIDs = Set(picks.map(\.id))
        func total(_ item: MossQuickItem) -> Double {
            var s = score(item.id, uses: uses, now: now)
            if context.queueHasItems && item.id == "action:needs-you" { s += boost }
            if context.anyInterrupted && item.id == "action:stuck" { s += boost }
            if context.usageHot && item.id == "cmd:/usage" { s += boost }
            return s
        }
        let rest = bar.enumerated()
            .filter { !pickedIDs.contains($0.element.id) }
            .map { (index: $0.offset, item: $0.element, score: total($0.element)) }
            .sorted { $0.score != $1.score ? $0.score > $1.score : $0.index < $1.index }
            .map(\.item)
        return picks + rest
    }

    /// Moss's picks, while younger than a day: cc-moss's pin (newest, even
    /// from a reply that wasn't shown), or for an older cc-moss the newest
    /// message that has them.
    public static func mossPicks(pin: MossChat.Pin? = nil, in messages: [MossMessage], now: Date) -> [String] {
        let picked: (items: [String], at: Date?)
        if let pin {
            picked = (pin.items, pin.at)
        } else if let m = messages.last(where: { $0.from == .moss && !$0.quick.isEmpty }) {
            picked = (m.quick, m.at)
        } else {
            return []
        }
        if let at = picked.at, now.timeIntervalSince(at) > 86_400 { return [] }
        var seen = Set<String>()
        return picked.items.compactMap(id(fromMoss:)).filter { seen.insert($0).inserted }
    }

    /// What "⋯" lists: everything the bar couldn't show, each once, in rank order.
    public static func overflow(_ ranked: [MossQuickItem], shown: Int) -> [MossQuickItem] {
        Array(ranked.dropFirst(max(0, shown)))
    }

    /// The last /usage result showed a limit at 80% or more.
    public static func usageHot(in messages: [MossMessage]) -> Bool {
        guard let m = messages.last(where: { $0.command == "/usage" && !$0.refused }),
              let usage = MossUsage.parse(m.text) else { return false }
        return usage.limits.contains { $0.percent >= 80 }
    }
}

/// The per-viewer use log behind the ranking, kept in UserDefaults.
public enum MossQuickLog {
    public static let key = "mossQuickUses"
    public static let cap = 200

    public static func load(_ defaults: UserDefaults = .standard) -> [MossQuickUse] {
        guard let data = defaults.data(forKey: key) else { return [] }
        return (try? JSONDecoder().decode([MossQuickUse].self, from: data)) ?? []
    }

    /// The log with one more use, newest last, the oldest dropped past the cap.
    public static func adding(_ id: String, at: Date, to uses: [MossQuickUse]) -> [MossQuickUse] {
        Array((uses + [MossQuickUse(id: id, at: at)]).suffix(cap))
    }

    public static func save(_ uses: [MossQuickUse], _ defaults: UserDefaults = .standard) {
        if let data = try? JSONEncoder().encode(uses) { defaults.set(data, forKey: key) }
    }
}
