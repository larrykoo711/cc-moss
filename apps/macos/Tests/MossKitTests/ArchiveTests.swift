import Foundation
import Testing
@testable import MossKit

@Suite struct ArchiveTests {
    let list = sessions([
        sessionJSON(id: "A", name: "live", status: "running"),
        with(sessionJSON(id: "B", name: "old-one", status: "closed", lastActive: "2026-08-01T10:00:00Z"),
             ["archived_at": "2026-09-01T03:00:00Z"]),
        with(sessionJSON(id: "C", name: "older", status: "interrupted", lastActive: "2026-07-01T10:00:00Z"),
             ["archived_at": "2026-09-02T03:00:00Z"]),
        sessionJSON(id: "D", name: "broken", status: "interrupted"),
    ])

    @Test func archivedAtIsReadAndNullMeansInTheList() {
        #expect(list[0].archivedAt == nil)
        #expect(!list[0].isArchived)
        #expect(list[1].archivedAt == parseTimestamp("2026-09-01T03:00:00Z"))
        #expect(list[1].isArchived)
    }

    @Test func aBadArchivedAtDoesNotCostTheSession() {
        let s = sessions([with(sessionJSON(id: "X"), ["archived_at": 42])])
        #expect(s.count == 1 && s[0].archivedAt == nil)
    }

    @Test func splitTakesArchivedOutNewestArchivedFirst() {
        let split = Archive.split(list)
        #expect(split.open.map(\.id) == ["A", "D"])
        #expect(split.archived.map(\.id) == ["C", "B"])
    }

    @Test func onlySessionsThatAreNotRunningCanBeArchived() {
        #expect(!Archive.allowed(list[0]), "running: end it first")
        #expect(Archive.allowed(list[3]))
        #expect(!Archive.allowed(list[1]), "already archived")
    }

    @Test func mossIsNeverArchived() {
        var moss = list[3]
        moss.role = "moss"
        #expect(!Archive.allowed(moss))
    }

    @Test func commandsKeepNamesAsNames() {
        #expect(Archive.arguments(targets: ["-odd"]) == ["archive", "--json", "--", "-odd"])
        #expect(Archive.unarchiveArguments(targets: ["x"]) == ["unarchive", "--json", "--", "x"])
    }

    @Test func cleanUpTextsInChinese() {
        L.$override.withValue(.zh) {
            #expect(Archive.cleanUpText("fix-invoice") == "清理归档会话：fix-invoice")
            #expect(Archive.cleanUpAllText == "清理全部归档会话")
        }
    }

    @Test func cleanUpTextsInEnglish() {
        L.$override.withValue(.en) {
            #expect(Archive.cleanUpText("fix-invoice") == "Clean up archived session: fix-invoice")
            #expect(Archive.cleanUpAllText == "Clean up all archived sessions")
        }
    }

    @Test func batchResultsDecode() throws {
        let data = Data(#"{"schema":1,"results":[{"name":"a","session_id":"s1","result":"archived"},{"name":"b","session_id":"s2","result":"refused","reason":"running"}]}"#.utf8)
        let r = try BatchResult.decode(data)
        #expect(r == [BatchResult(name: "a", sessionId: "s1", outcome: "archived"),
                      BatchResult(name: "b", sessionId: "s2", outcome: "refused", reason: "running")])
        L.$override.withValue(.en) {
            #expect(Archive.summary([r[1]]) == "b is still running. End it first.")
        }
    }

    @Test func aNewerResultSchemaIsSaidSo() {
        #expect(throws: MossClient.Failure.newerSchema(2)) {
            try BatchResult.decode(Data(#"{"schema":2,"results":[]}"#.utf8))
        }
    }

    @Test func archiveReadsARefusalFromStdoutEvenOnExit1() async throws {
        let runner = Recorder(status: 1, stdout: #"{"schema":1,"results":[{"name":"x","session_id":"s","result":"refused","reason":"running"}]}"#)
        let r = try await MossClient(executable: "/bin/cc-moss", runner: runner).archive(["x"])
        #expect(r.first?.outcome == "refused")
        #expect(await runner.calls == [["archive", "--json", "--", "x"]])
    }

    @Test func unarchiveRunsItsCommand() async throws {
        let runner = Recorder(stdout: #"{"schema":1,"results":[{"name":"x","session_id":"s","result":"unarchived"}]}"#)
        let r = try await MossClient(executable: "/bin/cc-moss", runner: runner).unarchive(["x"])
        #expect(r.first?.outcome == "unarchived")
        #expect(await runner.calls == [["unarchive", "--json", "--", "x"]])
    }

    private func layout(expanded: Bool) -> [SessionListItem] {
        let split = Archive.split(list)
        return SessionList.items(pinned: [], projects: Grouping.projects(split.open, home: "/home/u"), older: [],
                                 olderExpanded: false, archived: split.archived, archivedExpanded: expanded)
    }

    @Test func theArchivedGroupSitsLastFoldedWithItsCount() {
        let items = layout(expanded: false)
        #expect(items.last == .archivedHeader(2))
        #expect(!items.map(\.id).contains("B"))
    }

    @Test func theArchivedGroupOpensToItsRows() {
        #expect(Array(layout(expanded: true).map(\.id).suffix(3)) == ["header:archived", "C", "B"])
    }

    @Test func noArchivedNoGroup() {
        let items = SessionList.items(pinned: [], projects: Grouping.projects([list[0]]), older: [], olderExpanded: false)
        #expect(!items.map(\.id).contains("header:archived"))
    }

    @Test func searchShowsArchivedMatchesInTheirOwnGroup() {
        let split = Archive.split(list)
        let live = SearchIndex(split.open).search("live")
        let archived = SearchIndex(split.archived).search("old")
        let items = SessionList.searchItems(query: "old", live: live, archived: archived, closed: [])
        #expect(items.map(\.id) == ["ask-moss", "A", "header:archived-results", "B", "C"])
    }

    @Test func archivedMatchesAloneAreStillResults() {
        let archived = SearchIndex(Archive.split(list).archived).search("older")
        let items = SessionList.searchItems(query: "older", live: [], archived: archived, closed: [])
        #expect(items.map(\.id) == ["ask-moss", "header:archived-results", "C"])
    }

    @Test func endAndArchiveRunsArchiveEnd() async throws {
        #expect(Archive.endArguments(targets: ["-odd"]) == ["archive", "--end", "--json", "--", "-odd"])
        let runner = Recorder(status: 1, stdout: #"{"schema":1,"results":[{"name":"x","session_id":"s","result":"refused","reason":"busy"}]}"#)
        let r = try await MossClient(executable: "/bin/cc-moss", runner: runner).archiveEnding(["x"])
        #expect(await runner.calls == [["archive", "--end", "--json", "--", "x"]])
        L.$override.withValue(.zh) { #expect(Archive.summary(r) == "它正在干活，等这轮做完再归档") }
        L.$override.withValue(.en) { #expect(Archive.summary(r) == "It's working right now. Archive it when this turn is done.") }
    }

    @Test func endAndArchiveAsksInPlainWords() {
        L.$override.withValue(.zh) {
            #expect(Archive.endQuestion("api") + Archive.endHint == "结束「api」并收进归档？对话会留着，拿回来后可以找回接着聊。")
        }
        L.$override.withValue(.en) {
            #expect(Archive.endQuestion("api") + " " + Archive.endHint
                == "End “api” and put it in Archived? The conversation stays; bring it back to pick it up again.")
        }
    }
}
