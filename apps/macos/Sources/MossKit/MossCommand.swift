import Foundation

/// A Claude Code command Moss runs for you from the panel (T2.3 AMEND-2).
/// One list for the whole app: the suggestions under the text field and
/// what counts as a command. cc-moss itself decides what it will run.
public struct MossCommand: Identifiable, Equatable, Sendable {
    /// `/usage`, with the slash.
    public var name: String
    var details: (en: String, zh: String)

    public var id: String { name }

    public static func == (a: MossCommand, b: MossCommand) -> Bool { a.name == b.name }

    /// One line on what it shows.
    public func detail(_ lang: Lang = L.lang) -> String { lang == .zh ? details.zh : details.en }

    public static let all: [MossCommand] = [
        MossCommand(name: "/usage", details: ("Usage", "用量")),
        MossCommand(name: "/status", details: ("Account and version", "账号和版本")),
        MossCommand(name: "/context", details: ("Context used", "上下文用了多少")),
        MossCommand(name: "/cost", details: ("Cost so far", "这次花了多少")),
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

    /// What to offer while you type: a slash and no space yet, matched by
    /// prefix in the list's order. Anything else offers nothing.
    public static func suggestions(for text: String) -> [MossCommand] {
        guard text.hasPrefix("/"), !text.contains(where: \.isWhitespace) else { return [] }
        let typed = text.lowercased()
        return all.filter { $0.name.hasPrefix(typed) }
    }

    /// Text shown as a command (monospaced): it starts with a slash.
    public static func looksLikeCommand(_ text: String) -> Bool {
        text.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("/")
    }
}
