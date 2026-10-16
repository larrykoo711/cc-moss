import Foundation
import Testing
@testable import MossKit

/// Remembers every command it was asked to run and answers from a script.
actor RecordingRunner: CommandRunner {
    private(set) var calls: [[String]] = []
    private var replies: [CommandResult]

    init(_ replies: [CommandResult]) { self.replies = replies }

    nonisolated func run(_ executable: String, _ arguments: [String], timeout: Double) async throws -> CommandResult {
        await record(arguments)
    }

    private func record(_ arguments: [String]) -> CommandResult {
        calls.append(arguments)
        return replies.isEmpty ? CommandResult(status: 0, stdout: Data(), stderr: "") : replies.removeFirst()
    }
}

private func reply(_ json: String, status: Int32 = 0) -> CommandResult {
    CommandResult(status: status, stdout: Data(json.utf8), stderr: "")
}

private let listJSON = """
{
  "schema": 1,
  "ok": true,
  "dir": "/Users/demo/.local/bin",
  "entry": "/opt/homebrew/bin/cc-moss",
  "config": "/Users/demo/.config/cc-moss/config.toml",
  "dir_on_path": true,
  "aliases": [
    { "name": "t", "command": "pick", "status": "ok", "link": "ok", "shadow": "none", "detail": null, "fix": null },
    { "name": "tn", "command": "new --detach", "status": "shadowed", "link": "ok", "shadow": "function",
      "detail": "hidden by a shell function of the same name", "fix": "remove that `tn` from your shell's startup files" },
    { "name": "x", "command": "pick", "status": "something-newer", "link": "ok", "shadow": "none", "detail": null, "fix": null }
  ]
}
"""

@Suite struct AliasTests {
    @Test func decodesTheList() throws {
        let list = try AliasList.decode(Data(listJSON.utf8))
        #expect(list.dir == "/Users/demo/.local/bin")
        #expect(list.entry == "/opt/homebrew/bin/cc-moss")
        #expect(list.dirOnPath)
        #expect(list.aliases.map(\.name) == ["t", "tn", "x"])
        #expect(list.aliases[0].status == .ok)
        #expect(list.aliases[0].status.isFine)
        #expect(list.aliases[1].status == .shadowed)
        #expect(list.aliases[1].detail == "hidden by a shell function of the same name")
        #expect(list.aliases[2].status == .unknown, "a status from a newer cc-moss still decodes")
    }

    @Test func refusesANewerSchema() {
        let newer = listJSON.replacingOccurrences(of: "\"schema\": 1", with: "\"schema\": 2")
        #expect(throws: SnapshotError.newerSchema(2)) { try AliasList.decode(Data(newer.utf8)) }
    }

    @Test func searchMatchesNameAndCommand() throws {
        let list = try AliasList.decode(Data(listJSON.utf8))
        #expect(list.filtered("").count == 3)
        #expect(list.filtered("detach").map(\.name) == ["tn"])
        #expect(list.filtered("PICK").map(\.name) == ["t", "x"])
        #expect(list.filtered("new tn").map(\.name) == ["tn"])
        #expect(list.filtered("nothing").isEmpty)
    }

    @Test func savingANameThatExistsAsksFirst() throws {
        let list = try AliasList.decode(Data(listJSON.utf8))
        #expect(list.replaces("tn", editing: nil), "adding a name that exists")
        #expect(list.replaces("tn", editing: "t"), "renaming onto another alias")
        #expect(!list.replaces("tn", editing: "tn"), "editing a row in place")
        #expect(!list.replaces("fresh", editing: nil))
        #expect(AliasFailure.taken("tn").canReplace)
        #expect(!AliasFailure.taken("tn").canForce)
    }

    @Test func argumentsKeepTypedTextOutOfOptions() {
        #expect(AliasArgs.list() == ["alias", "ls", "--json"])
        #expect(AliasArgs.add(name: "tn", command: "new --detach")
                == ["alias", "add", "--json", "--", "tn", "new --detach"])
        #expect(AliasArgs.add(name: "tn", command: "new", force: true)
                == ["alias", "add", "--json", "--force", "--", "tn", "new"])
        // Whatever is typed stays one argument after `--`: no shell, no options.
        #expect(AliasArgs.add(name: "--force", command: "pick; rm -rf ~ $(id)")
                == ["alias", "add", "--json", "--", "--force", "pick; rm -rf ~ $(id)"])
        #expect(AliasArgs.remove(name: "-x") == ["alias", "rm", "--json", "--", "-x"])
    }

    @Test func clientListsThroughTheCLI() async throws {
        let runner = RecordingRunner([reply(listJSON)])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        let list = try await client.aliases()
        #expect(list.aliases.count == 3)
        #expect(await runner.calls == [["alias", "ls", "--json"]])
    }

    @Test func addReturnsTheAliasAndSurfacesRefusals() async throws {
        let ok = """
        {"schema":1,"ok":true,"updated":false,"dir_on_path":true,
         "alias":{"name":"tn","command":"new","status":"ok","link":"ok","shadow":"none","detail":null,"fix":null}}
        """
        let refused = """
        {"schema":1,"ok":false,"code":"exists","error":"`ls` is already a command (/bin/ls)."}
        """
        let runner = RecordingRunner([reply(ok), reply(refused, status: 1)])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        let added = try await client.addAlias(name: "tn", command: "new")
        #expect(added == Alias(name: "tn", command: "new", status: .ok))
        do {
            try await client.addAlias(name: "ls", command: "pick")
            Issue.record("a refusal must throw")
        } catch let failure as AliasFailure {
            #expect(failure.code == "exists")
            #expect(failure.canForce)
            #expect(failure.message.contains("/bin/ls"))
        }
    }

    @Test func renameAddsBeforeRemoving() async throws {
        let ok = #"{"schema":1,"ok":true,"alias":{"name":"tt","command":"pick","status":"ok"}}"#
        let runner = RecordingRunner([reply(ok), reply(#"{"schema":1,"ok":true,"name":"t"}"#)])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        try await client.renameAlias(from: "t", to: "tt", command: "pick")
        #expect(await runner.calls == [
            ["alias", "add", "--json", "--", "tt", "pick"],
            ["alias", "rm", "--json", "--", "t"],
        ])
    }

    @Test func refusedRenameLeavesTheOldAlias() async throws {
        let refused = #"{"schema":1,"ok":false,"code":"bad_name","error":"`T` is not allowed"}"#
        let runner = RecordingRunner([reply(refused, status: 1)])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        await #expect(throws: AliasFailure(code: "bad_name", message: "`T` is not allowed")) {
            try await client.renameAlias(from: "t", to: "T", command: "pick")
        }
        #expect(await runner.calls.count == 1, "no rm after a refused add")
    }

    @Test func anOlderCLIWithoutAliasesIsACommandFailure() async {
        let runner = RecordingRunner([CommandResult(status: 2, stdout: Data(), stderr: "error: unrecognized subcommand 'alias'\n")])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        await #expect(throws: MossClient.Failure.command("error: unrecognized subcommand 'alias'")) {
            _ = try await client.aliases()
        }
    }
}
