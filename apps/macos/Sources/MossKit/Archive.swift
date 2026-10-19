import Foundation

/// Archived sessions (T2.5): put away out of the list, still searchable,
/// brought back any time. Deleting them for good goes through Moss, who
/// asks you first; the app only hands Moss the request.
public enum Archive {
    /// The row offers "Archive": it isn't running (ended or interrupted),
    /// isn't Moss and isn't archived already.
    public static func allowed(_ s: Session) -> Bool {
        !s.isMoss && !s.isArchived && s.status != "running"
    }

    /// The list's sessions and the archived ones, apart. Archived are
    /// newest-archived first; Moss is never archived.
    public static func split(_ sessions: [Session]) -> (open: [Session], archived: [Session]) {
        let archived = sessions.filter { $0.isArchived && !$0.isMoss }
            .sorted { ($0.archivedAt ?? .distantPast, $0.lastActiveAt) > ($1.archivedAt ?? .distantPast, $1.lastActiveAt) }
        return (sessions.filter { !$0.isArchived || $0.isMoss }, archived)
    }

    /// `archive --json -- <name>…`; `--` keeps a name starting with "-" a name.
    public static func arguments(targets: [String]) -> [String] {
        ["archive", "--json", "--"] + targets
    }

    /// `unarchive --json -- <name>…`.
    public static func unarchiveArguments(targets: [String]) -> [String] {
        ["unarchive", "--json", "--"] + targets
    }

    /// What "Clean Up" sends Moss for one session.
    public static func cleanUpText(_ name: String) -> String {
        L.t("Clean up archived session: \(name)", "清理归档会话：\(name)")
    }

    /// What "Clean Up All" sends Moss.
    public static var cleanUpAllText: String {
        L.t("Clean up all archived sessions", "清理全部归档会话")
    }

    /// A short line for the action bar once archiving is done.
    public static func summary(_ results: [BatchResult]) -> String {
        if results.count == 1, let r = results.first {
            return switch r.outcome {
            case "archived": L.t("\(r.name) is in Archived now.", "\(r.name) 收进已归档了。")
            case "unarchived": L.t("\(r.name) is back in the list.", "\(r.name) 回到列表了。")
            default: refusal(r)
            }
        }
        let done = results.filter { $0.outcome == "archived" || $0.outcome == "unarchived" }.count
        return L.t("Done: \(done) of \(results.count).", "完成了 \(done) 个，共 \(results.count) 个。")
    }

    /// Why one wasn't archived, in words.
    public static func refusal(_ r: BatchResult) -> String {
        switch r.reason {
        case "running": L.t("\(r.name) is still running. End it first.", "\(r.name) 还在运行，先结束它。")
        case "moss": L.t("Moss stays in the list.", "Moss 不能归档。")
        default: L.t("\(r.name) didn't change.", "\(r.name) 没有变化。")
        }
    }
}

extension MossClient {
    /// `cc-moss archive --json -- <name>…`.
    public func archive(_ targets: [String]) async throws -> [BatchResult] {
        try await batch(Archive.arguments(targets: targets))
    }

    /// `cc-moss unarchive --json -- <name>…`.
    public func unarchive(_ targets: [String]) async throws -> [BatchResult] {
        try await batch(Archive.unarchiveArguments(targets: targets))
    }
}
