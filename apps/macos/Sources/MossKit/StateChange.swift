import Foundation

/// Which changes in cc-moss's state folder are worth refreshing for, and
/// how soon. Everything else there changes all the time: SQLite makes and
/// removes `registry.db-wal` / `-shm` whenever anyone opens the registry
/// (a refresh too, which would wake the next one), the app's own heartbeat
/// for answering questions, logs, and the daemon's bookkeeping.
public enum StateChange {
    /// - announced: the folder has `registry.changed` (a cc-moss that touches
    ///   it); with an older one the database file itself counts.
    public static func matters(_ path: String, in stateDir: String, announced: Bool = true) -> Bool {
        let base = stateDir.hasSuffix("/") ? stateDir : stateDir + "/"
        guard path.hasPrefix(base) else { return false }
        let rel = String(path.dropFirst(base.count))
        // cc-moss touches `registry.changed` whenever it wrote the registry;
        // the database itself may not change until SQLite folds its log in.
        if rel == "registry.changed" || rel == "moss/chat.jsonl" { return true }
        if rel == "registry.db" { return !announced }
        // A question written, answered or withdrawn; not who's listening.
        return rel.hasPrefix("asks/") && !rel.hasPrefix("asks/takers/") && rel.hasSuffix(".json")
    }

    /// The least time between two refreshes: quick while you're looking at
    /// the panel, gentler while only the menu bar and Mo are listening.
    public static func gap(panelShown: Bool) -> TimeInterval { panelShown ? 1.5 : 5 }
}
