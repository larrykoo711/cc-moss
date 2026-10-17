import Foundation

/// Settings › Aliases, the everyday way: a few things people do, each with
/// one short name to type in the terminal. The command behind each stays
/// out of sight; other aliases (made by hand) are "custom".
public enum AliasBoard {
    /// Something to give a short name to.
    public struct Task: Identifiable, Equatable, Sendable {
        public let id: String
        /// What runs after `cc-moss`.
        public let command: String
        public let icon: String
        public var title: String { AliasBoard.title(id) }
        /// What happens when you type the name, for the hover preview.
        public var result: String { AliasBoard.result(id) }
        /// A name to suggest in the empty field.
        public let example: String
    }

    public static let tasks: [Task] = [
        Task(id: "pick", command: "pick", icon: "list.bullet.rectangle", example: "t"),
        Task(id: "new", command: "new", icon: "plus.rectangle", example: "tn"),
        Task(id: "trusted", command: "new --dangerously-skip-permissions", icon: "bolt", example: "tt"),
        Task(id: "restore", command: "restore --yes", icon: "arrow.counterclockwise", example: "tb"),
        Task(id: "doctor", command: "doctor", icon: "stethoscope", example: "tdoc"),
    ]

    static func title(_ id: String) -> String {
        switch id {
        case "pick": L.t("Open the session list", "打开会话列表")
        case "new": L.t("Start a new session here", "在这里开一个新会话")
        case "trusted": L.t("New session, no step-by-step OKs", "新会话，不用一步步点头")
        case "restore": L.t("Bring back every stopped session", "把断了的会话全找回")
        default: L.t("Check that everything works", "检查一切是否正常")
        }
    }

    static func result(_ id: String) -> String {
        switch id {
        case "pick": L.t("The list opens; Return goes in", "会话列表打开，回车进去")
        case "new": L.t("A session starts in this folder", "在当前目录开好一个会话")
        case "trusted": L.t("Claude goes ahead without asking each time", "Claude 做事不再逐条问你")
        case "restore": L.t("Every stopped session is back", "断了的会话全都回来了")
        default: L.t("Hooks, tmux and aliases, checked", "钩子、tmux、别名都检查一遍")
        }
    }

    public struct Board: Equatable, Sendable {
        /// Each task with its alias, if it has one.
        public var rows: [(task: Task, alias: Alias?)]
        /// Aliases that are not one of the tasks.
        public var custom: [Alias]

        public static func == (a: Board, b: Board) -> Bool {
            a.rows.map { $0.task.id + ($0.alias?.name ?? "") } == b.rows.map { $0.task.id + ($0.alias?.name ?? "") }
                && a.custom == b.custom
        }
    }

    /// Spaces don't matter: `new  --detach` is `new --detach`.
    static func same(_ a: String, _ b: String) -> Bool {
        a.split(whereSeparator: \.isWhitespace) == b.split(whereSeparator: \.isWhitespace)
    }

    public static func board(_ aliases: [Alias]) -> Board {
        var rest = aliases
        var rows: [(task: Task, alias: Alias?)] = []
        for task in tasks {
            if let i = rest.firstIndex(where: { same($0.command, task.command) }) {
                rows.append((task, rest.remove(at: i)))
            } else {
                rows.append((task, nil))
            }
        }
        return Board(rows: rows, custom: rest)
    }

    /// What saving the field does.
    public enum Change: Equatable, Sendable {
        case nothing
        case add(name: String)
        case rename(from: String, to: String)
        case remove(name: String)
    }

    public static func change(field: String, current: Alias?) -> Change {
        let name = field.trimmingCharacters(in: .whitespaces)
        switch (current, name.isEmpty) {
        case (nil, true): return .nothing
        case (nil, false): return .add(name: name)
        case let (alias?, true): return .remove(name: alias.name)
        case let (alias?, false): return alias.name == name ? .nothing : .rename(from: alias.name, to: name)
        }
    }

    /// A custom alias typed as one line: `name = command`.
    public static func parseCustom(_ line: String) -> (name: String, command: String)? {
        guard let eq = line.firstIndex(of: "=") else { return nil }
        let name = line[..<eq].trimmingCharacters(in: .whitespaces)
        var command = line[line.index(after: eq)...].trimmingCharacters(in: .whitespaces)
        if command.hasPrefix("cc-moss ") { command = String(command.dropFirst("cc-moss ".count)) }
        guard !name.isEmpty, !command.isEmpty, !name.contains(" ") else { return nil }
        return (name, command)
    }
}
