import Foundation

/// Sessions that share a folder, shown under one heading like the picker does.
public struct Project: Equatable, Identifiable, Sendable {
    public var path: String
    /// The heading: the folder name, or a short path when two folders share a name.
    public var label: String
    public var worktree: Session.Worktree?
    public var sessions: [Session]

    public var id: String { path }
}

public enum Grouping {
    /// Moss apart from everything else. Moss is the one pinned row at the
    /// top of the panel (the running one, newest if there are two); its old
    /// records are nobody's business: they are never grouped, counted,
    /// offered for restore or dismissed.
    public static func splitMoss(_ sessions: [Session]) -> (moss: Session?, others: [Session]) {
        let moss = sessions
            .filter { $0.isMoss && $0.status == "running" }
            .max { $0.lastActiveAt < $1.lastActiveAt }
        return (moss, sessions.filter { !$0.isMoss })
    }

    /// What the menu bar icon and its count look at: the sessions, and Moss
    /// while it needs you. Moss is never counted as a session, but a Moss
    /// waiting on a permission or a question must reach you like any other.
    public static func alerting(_ sessions: [Session], moss: Session?) -> [Session] {
        guard let moss, moss.needsYou else { return sessions }
        return sessions + [moss]
    }

    /// Group open sessions by folder. Projects and the sessions in them are
    /// ordered by most recent activity; closed sessions and Moss are left out.
    public static func projects(_ sessions: [Session], home: String? = NSHomeDirectory()) -> [Project] {
        var order: [String] = []
        var byPath: [String: [Session]] = [:]
        for s in sessions where s.state != .closed && !s.isMoss {
            if byPath[s.cwd] == nil { order.append(s.cwd) }
            byPath[s.cwd, default: []].append(s)
        }
        var projects = order.map { path -> Project in
            let items = byPath[path]!.sorted { $0.lastActiveAt > $1.lastActiveAt }
            return Project(path: path, label: "", worktree: items.first?.worktree, sessions: items)
        }
        projects.sort { ($0.sessions.first?.lastActiveAt ?? .distantPast) > ($1.sessions.first?.lastActiveAt ?? .distantPast) }
        let bases = projects.map { basename($0.path, home: home) }
        for i in projects.indices {
            let base = bases[i]
            projects[i].label = bases.filter { $0 == base }.count > 1
                ? shortPath(projects[i].path, home: home)
                : base
        }
        return projects
    }

    /// `~/a/b` for paths under the home folder.
    public static func tilde(_ path: String, home: String?) -> String {
        guard let home, !home.isEmpty, home != "/" else { return path }
        if path == home { return "~" }
        if path.hasPrefix(home + "/") { return "~" + path.dropFirst(home.count) }
        return path
    }

    static func basename(_ path: String, home: String?) -> String {
        let t = tilde(path, home: home)
        let last = t.split(separator: "/").last.map(String.init)
        return last?.isEmpty == false ? last! : t
    }

    /// The whole path when short, else `…/parent/name` (the picker's rule).
    static func shortPath(_ path: String, home: String?) -> String {
        let t = tilde(path, home: home)
        if t.count <= 28 { return t }
        let parts = t.split(separator: "/", omittingEmptySubsequences: true)
        if parts.count < 2 { return t }
        return "…/\(parts[parts.count - 2])/\(parts[parts.count - 1])"
    }
}
