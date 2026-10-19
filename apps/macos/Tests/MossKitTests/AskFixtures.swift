import Foundation
@testable import MossKit

/// A question as `ask list --json` prints it.
func askJSON(id: String = "a1b2c3d4e5f6a7b8", kind: String = "question", session: String = "01A-sid-0000",
             name: String? = "job-a", questions: [[String: Any]]? = nil,
             tool: [String: Any]? = nil) -> [String: Any] {
    var a: [String: Any] = [
        "schema": 1, "id": id, "kind": kind, "session_id": session, "prompt_id": "p1", "cwd": "/home/u/projects/acme",
        "created_at": "2026-10-18T10:00:00Z", "expires_at": "2026-10-18T10:05:00Z",
    ]
    if let name { a["session_name"] = name }
    if kind == "question" {
        a["questions"] = questions ?? [questionJSON()]
    } else {
        a["tool"] = tool ?? ["name": "Bash", "preview": "rm -rf build/", "input_hash": "00ff"]
    }
    return a
}

func questionJSON(_ text: String = "Which database?", header: String = "Database",
                  labels: [String] = ["Postgres", "SQLite"], multi: Bool = false) -> [String: Any] {
    ["question": text, "header": header, "multiSelect": multi,
     "options": labels.map { ["label": $0, "description": "about \($0)"] }]
}

func askListData(_ asks: [[String: Any]], schema: Int = 1) -> Data {
    try! JSONSerialization.data(withJSONObject: ["schema": schema, "asks": asks])
}

func asks(_ list: [[String: Any]]) -> [Ask] {
    try! AskList.decode(askListData(list))
}

func ask(_ json: [String: Any] = askJSON()) -> Ask {
    asks([json])[0]
}

/// Records every command and answers each from a table (first matching prefix wins).
final class AskRunner: CommandRunner, @unchecked Sendable {
    private let lock = NSLock()
    private var _calls: [[String]] = []
    var replies: [(prefix: [String], result: CommandResult)]

    init(_ replies: [(prefix: [String], result: CommandResult)] = []) {
        self.replies = replies
    }

    var calls: [[String]] { lock.withLock { _calls } }

    func run(_ executable: String, _ arguments: [String], timeout: Double) async throws -> CommandResult {
        lock.withLock { _calls.append(arguments) }
        for r in replies where arguments.starts(with: r.prefix) { return r.result }
        return CommandResult(status: 0, stdout: Data(), stderr: "")
    }
}

func ok(_ out: String) -> CommandResult { CommandResult(status: 0, stdout: Data(out.utf8), stderr: "") }
func fail(_ err: String) -> CommandResult { CommandResult(status: 1, stdout: Data(), stderr: err) }
