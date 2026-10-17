import Foundation

/// Starting a named session from the panel: `cc-moss new <name> -C <dir>
/// --detach`, then open it in the terminal. One open session per name: a
/// running one is opened instead, an interrupted one is offered back.
public enum NewSession {
    /// What pressing "Create" leads to.
    public enum Plan: Equatable, Sendable {
        /// Run cc-moss with these arguments, then open the new session.
        case create([String])
        /// A session of that name is running: open it, start nothing.
        case open(Session)
        /// A session of that name stopped: ask "bring it back?" first.
        case askRestore(Session)
        /// The name can't be used; why, in words.
        case invalid(String)
    }

    /// What the name field says while you type.
    public enum Hint: Equatable, Sendable {
        case free
        case running(Session)
        case interrupted(Session)
        case invalid(String)
    }

    /// Up to `limit` folders sessions worked in, most recent first.
    public static func recentProjects(_ sessions: [Session], limit: Int = 10) -> [String] {
        var seen = Set<String>()
        return sessions
            .sorted { $0.lastActiveAt > $1.lastActiveAt }
            .map(\.cwd)
            .filter { seen.insert($0).inserted }
            .prefix(limit)
            .map { $0 }
    }

    /// The folder the form starts with: your workspace when you set one
    /// (and it is still there), else where the last activity was.
    public static func defaultDirectory(_ sessions: [Session], workspace: String? = nil,
                                        exists: (String) -> Bool = { FileManager.default.fileExists(atPath: $0) }) -> String? {
        if let workspace, !workspace.isEmpty, exists(workspace) { return workspace }
        return recentProjects(sessions, limit: 1).first
    }

    /// Settings › General: where new sessions start. Unset: the most recent folder.
    public static let workspaceKey = "newSessionWorkspace"

    /// The name the form suggests: the folder's name, like `cc-moss new` does.
    public static func defaultName(for directory: String) -> String {
        let last = (directory as NSString).lastPathComponent
        return last.isEmpty || last == "/" ? "session" : last
    }

    /// The arguments for cc-moss. `restore` adds `--yes`: the name belongs
    /// to an interrupted session and the user said bring it back (T1.5).
    public static func arguments(name: String, directory: String, restore: Bool = false) -> [String] {
        ["new", name, "-C", directory, "--detach"] + (restore ? ["--yes"] : [])
    }

    /// Why `name` can't be used, or nil.
    public static func problem(with name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return L.t("Give it a name.", "给它起个名字。") }
        // `new -x` would read as an option, and `new -- x` hands x to Claude.
        if trimmed.hasPrefix("-") { return L.t("A name can't start with “-”.", "名字不能以「-」开头。") }
        if trimmed.unicodeScalars.contains(where: { $0.properties.generalCategory == .control }) {
            return L.t("No control characters, please.", "名字里不能有控制字符。")
        }
        return nil
    }

    /// The open session already called `name`, if any.
    public static func hint(name: String, sessions: [Session]) -> Hint {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if let problem = problem(with: trimmed) { return .invalid(problem) }
        let same = sessions.filter { $0.name == trimmed && $0.state != .closed }
        if let running = same.first(where: { $0.state == .working || $0.state == .waiting }) {
            return .running(running)
        }
        if let stopped = same.first(where: { $0.state == .interrupted }) {
            return .interrupted(stopped)
        }
        return .free
    }

    public static func plan(name: String, directory: String, sessions: [Session]) -> Plan {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        switch hint(name: trimmed, sessions: sessions) {
        case .invalid(let why): return .invalid(why)
        case .running(let s): return .open(s)
        case .interrupted(let s): return .askRestore(s)
        case .free: return .create(arguments(name: trimmed, directory: directory))
        }
    }
}

extension MossClient {
    /// Run `cc-moss new …`; cc-moss's own words when it says no. Starting
    /// tmux and Claude Code takes a moment, hence the longer wait.
    public func newSession(_ arguments: [String]) async throws {
        _ = try await plainData(arguments, timeout: 30)
    }
}
