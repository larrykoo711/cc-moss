import Foundation
import Testing
@testable import MossKit

private let now = ISO8601DateFormatter().date(from: "2026-10-16T12:00:00Z")!

@Suite struct RestoreGroupingTests {
    let list = sessions([
        sessionJSON(id: "A", name: "busy", status: "running", busy: true, lastActive: "2026-10-16T11:00:00Z"),
        sessionJSON(id: "B", name: "fresh", status: "interrupted", lastActive: "2026-10-15T22:00:00Z"),
        // Six days and change: still recent.
        sessionJSON(id: "C", name: "week", status: "interrupted", lastActive: "2026-10-10T00:00:00Z"),
        sessionJSON(id: "D", name: "old", status: "interrupted", lastActive: "2026-10-01T09:00:00Z"),
        sessionJSON(id: "E", name: "older", status: "interrupted", lastActive: "2026-09-02T09:00:00Z"),
    ])

    @Test func recentStayInTheList() {
        let g = RestoreGroups(list, now: now)
        #expect(g.recent.map(\.id) == ["B", "C"])
        #expect(RestoreGroups.listed(list, now: now).map(\.id) == ["A", "B", "C"])
    }

    @Test func oldInterruptionsAreFolded() {
        let g = RestoreGroups(list, now: now)
        #expect(g.older.map(\.id) == ["D", "E"])
        #expect(!RestoreGroups.listed(list, now: now).contains { $0.id == "D" || $0.id == "E" })
    }

    @Test func runningSessionsNeverFold() {
        let stale = sessions([sessionJSON(id: "R", status: "running", lastActive: "2026-09-01T00:00:00Z")])
        #expect(RestoreGroups(stale, now: now).older.isEmpty)
        #expect(RestoreGroups.listed(stale, now: now).map(\.id) == ["R"])
    }

    @Test func countsOnlyRecent() {
        let c = StateCounts(list, now: now)
        #expect(c.interrupted == 2)
        #expect(c.olderInterrupted == 2)
        #expect(c.working == 1)
    }
}

private func reply(_ json: String, status: Int32 = 0, stderr: String = "") -> CommandResult {
    CommandResult(status: status, stdout: Data(json.utf8), stderr: stderr)
}

private let mixedJSON = """
{"schema":1,"results":[
  {"id":"01A","name":"job-a","ok":true},
  {"id":"01B","name":"job-b","ok":false,"reason":"empty","message":"skip: no messages were ever sent"},
  {"id":"01C","name":"job-c","ok":false,"reason":"something_new","message":"?"}
]}
"""

@Suite struct RestoreFlowTests {
    @Test func buildsCommand() async throws {
        #expect(RestoreFlow.arguments(targets: []) == ["restore", "--yes", "--json"])
        // `--` keeps a name starting with "-" from reading as an option.
        #expect(RestoreFlow.arguments(targets: ["job-a", "-odd"]) == ["restore", "--yes", "--json", "--", "job-a", "-odd"])

        let runner = RecordingRunner([reply(#"{"schema":1,"results":[]}"#)])
        _ = try await MossClient(executable: "/x/cc-moss", runner: runner).restore(["job-a"])
        #expect(await runner.calls == [["restore", "--yes", "--json", "--", "job-a"]])
    }

    @Test func targetsByNameElseConversationId() {
        let s = sessions([sessionJSON(id: "01A", name: "job-a"), sessionJSON(id: "01B", name: nil)])
        #expect(s[0].target == "job-a")
        #expect(s[1].target == "01B-sid-0000")
    }

    @Test func decodesResults() throws {
        let report = try RestoreReport.decode(Data(mixedJSON.utf8))
        #expect(report.restored.map(\.id) == ["01A"])
        #expect(report.failed.map(\.reason) == [.empty, .other])
        #expect(report.results[1].message == "skip: no messages were ever sent")
        #expect(RestoreReason.empty.isFinal && RestoreReason.missingDir.isFinal && !RestoreReason.tmuxFailed.isFinal)
    }

    @Test func newerSchemaIsRefused() {
        #expect(throws: SnapshotError.newerSchema(2)) {
            try RestoreReport.decode(Data(#"{"schema":2,"results":[]}"#.utf8))
        }
    }

    @Test func secondClickWhileRunningIsIgnored() {
        var flow = RestoreFlow()
        var ok: Bool
        ok = flow.begin(.one("01A")); #expect(ok)
        ok = flow.begin(.one("01A")); #expect(!ok, "same session again")
        ok = flow.begin(.one("01B")); #expect(ok, "another session is fine")
        ok = flow.begin(.all(["01A", "01B", "01C"])); #expect(!ok, "not while some are on their way")
        flow.finish(.one("01A"), report: RestoreReport(results: [.init(id: "01A", name: "job-a", ok: true)]))
        flow.finish(.one("01B"), report: RestoreReport(results: []))
        #expect(!flow.busy)

        ok = flow.begin(.all(["01A", "01C"])); #expect(ok)
        #expect(flow.isRunning("01C"))
        #expect(flow.isRunning("01Z"), "everything is in flight during bring-all-back")
        ok = flow.begin(.one("01C")); #expect(!ok)
        ok = flow.begin(.all(["01A"])); #expect(!ok)
        flow.finish(.all(["01A", "01C"]), report: RestoreReport(results: []))
        #expect(!flow.busy)
        ok = flow.begin(.one("01C")); #expect(ok)
    }

    @Test func failureCarriesTheReason() async throws {
        var flow = RestoreFlow()
        var ok: Bool
        let scope = RestoreFlow.Scope.all(["01A", "01B", "01C"])
        ok = flow.begin(scope); #expect(ok)
        flow.finish(scope, report: try RestoreReport.decode(Data(mixedJSON.utf8)))
        #expect(flow.failures["01A"] == nil)
        #expect(flow.failures["01B"]?.reason == .empty)
        #expect(flow.failures["01B"]?.message == "skip: no messages were ever sent")

        // cc-moss itself said no: its words reach the row.
        let runner = RecordingRunner([reply("", status: 1, stderr: "cc-moss: tmux is not installed\n")])
        await #expect(throws: MossClient.Failure.command("cc-moss: tmux is not installed")) {
            _ = try await MossClient(executable: "/x/cc-moss", runner: runner).restore([])
        }
        ok = flow.begin(.one("01C")); #expect(ok)
        flow.fail(.one("01C"), message: "cc-moss: tmux is not installed")
        #expect(flow.failures["01C"]?.message == "cc-moss: tmux is not installed")

        // A later try clears the old failure; leaving interrupted forgets it.
        ok = flow.begin(.one("01C")); #expect(ok)
        #expect(flow.failures["01C"] == nil)
        flow.prune(keeping: [])
        #expect(flow.failures.isEmpty)
    }
}
