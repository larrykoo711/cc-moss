import Foundation

/// How Claude Code is signed in, as `moss status --json` says (`account`).
public enum MossAccount: String, CaseIterable, Equatable, Sendable {
    /// A Claude subscription (Pro, Max).
    case subscription
    /// An Anthropic API key.
    case api
    /// A cloud provider (Bedrock, Vertex) or another third party.
    case cloud
    /// An older cc-moss, or it couldn't tell.
    case unknown
}

/// A Claude Code command Moss runs for you from the panel (T2.3 AMEND-2).
/// One list for the whole app: the suggestions under the text field and
/// what counts as a command. cc-moss itself decides what it will run.
public struct MossCommand: Identifiable, Equatable, Sendable {
    /// `/usage`, with the slash.
    public var name: String
    var details: (en: String, zh: String)
    /// The accounts it means something for; nil: every account.
    public var accounts: Set<MossAccount>?

    public var id: String { name }

    public static func == (a: MossCommand, b: MossCommand) -> Bool { a.name == b.name }

    /// One line on what it shows.
    public func detail(_ lang: Lang = L.lang) -> String { lang == .zh ? details.zh : details.en }

    public static let all: [MossCommand] = [
        MossCommand(name: "/usage", details: ("Usage", "用量"), accounts: [.subscription]),
        MossCommand(name: "/status", details: ("Account and version", "账号和版本")),
        MossCommand(name: "/context", details: ("Context used", "上下文用了多少")),
        MossCommand(name: "/cost", details: ("Cost so far", "这次花了多少"), accounts: [.api, .cloud]),
        MossCommand(name: "/stats", details: ("Stats", "使用统计")),
        MossCommand(name: "/help", details: ("Help", "帮助")),
        MossCommand(name: "/clear", details: ("Clear Moss's conversation", "清空 Moss 的对话")),
        MossCommand(name: "/compact", details: ("Compact Moss's conversation", "压缩 Moss 的对话")),
    ]

    /// Whether `name` (`/usage`, any case) is one Moss runs from the panel.
    public static func isAllowed(_ name: String) -> Bool {
        let n = name.trimmingCharacters(in: .whitespaces).lowercased()
        return all.contains { $0.name == n }
    }

    /// Whether it means something for `account`. Unknown: everything does.
    public func fits(_ account: MossAccount) -> Bool {
        guard account != .unknown, let accounts else { return true }
        return accounts.contains(account)
    }

    /// What to offer while you type: a slash and no space yet, matched by
    /// prefix in the list's order, only what fits the account. Anything else
    /// offers nothing. (Typing one that doesn't fit still sends it: Claude
    /// Code answers for itself.)
    public static func suggestions(for text: String, account: MossAccount = .unknown) -> [MossCommand] {
        guard text.hasPrefix("/"), !text.contains(where: \.isWhitespace) else { return [] }
        let typed = text.lowercased()
        return all.filter { $0.name.hasPrefix(typed) && $0.fits(account) }
    }

    /// Text shown as a command (monospaced): it starts with a slash.
    public static func looksLikeCommand(_ text: String) -> Bool {
        text.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("/")
    }
}
