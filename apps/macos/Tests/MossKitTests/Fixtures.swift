import Foundation
@testable import MossKit

/// A session as `ls --json` would print it.
func sessionJSON(
    id: String = "01A",
    name: String? = "job-a",
    status: String = "running",
    busy: Bool = false,
    cwd: String = "/home/u/projects/acme",
    lastActive: String = "2026-10-16T10:00:00.000Z",
    tmux: Bool = true,
    note: String? = nil,
    lastPrompt: String? = nil,
    worktree: [String: String]? = nil
) -> [String: Any] {
    var s: [String: Any] = [
        "id": id, "agent": "claude", "status": status, "session_id": "\(id)-sid-0000",
        "session_ids": ["\(id)-sid-0000"], "cwd": cwd, "pid": 42, "end_reason": NSNull(),
        "busy": busy, "notes": ["goal": NSNull(), "state": NSNull(), "note": (note as Any?) ?? NSNull()] as [String: Any],
        "created_at": "2026-10-16T09:00:00Z", "last_active_at": lastActive, "ended_at": NSNull(),
    ]
    s["name"] = (name as Any?) ?? NSNull()
    s["last_prompt"] = (lastPrompt as Any?) ?? NSNull()
    let loc: [String: Any] = ["socket": "/tmp/tmux-1/default", "session_id": "$1", "session_name": "host-a_\(name ?? "x")", "pane_id": "%1"]
    s["tmux"] = tmux ? loc : NSNull()
    s["worktree"] = (worktree as Any?) ?? NSNull()
    return s
}

func snapshotData(_ sessions: [[String: Any]], schema: Int = 1, stateDir: String? = "/home/u/.local/state/cc-moss") -> Data {
    var top: [String: Any] = ["schema": schema, "sessions": sessions]
    if let stateDir { top["state_dir"] = stateDir }
    return try! JSONSerialization.data(withJSONObject: top)
}

func sessions(_ list: [[String: Any]]) -> [Session] {
    try! Snapshot.decode(snapshotData(list)).sessions
}
