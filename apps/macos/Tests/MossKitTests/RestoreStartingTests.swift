import Foundation
import Testing
@testable import MossKit

@Suite struct RestoreStartingTests {
    @Test func acceptedLaunchIsNotRestoredOrFailed() throws {
        let report = try RestoreReport.decode(Data(#"{"schema":1,"results":[{"id":"A","name":"demo","ok":true,"result":"starting"}]}"#.utf8))
        #expect(report.restored.isEmpty)
        #expect(report.failed.isEmpty)
        #expect(report.starting.map(\.id) == ["A"])
        #expect(report.starting.first?.result == "starting")
    }

    @Test func legacyOKStillMeansRestored() throws {
        let report = try RestoreReport.decode(Data(#"{"schema":1,"results":[{"id":"A","name":"legacy","ok":true}]}"#.utf8))
        #expect(report.restored.map(\.id) == ["A"])
    }

    @Test func mixedResultsCountOnlyConfirmedAndLegacyRestores() throws {
        let report = try RestoreReport.decode(Data(#"{"schema":1,"results":[{"id":"A","name":"ready","ok":true,"result":"restored"},{"id":"B","name":"pending","ok":true,"result":"starting"},{"id":"C","name":"legacy","ok":true},{"id":"D","name":"failed","ok":false,"reason":"empty"}]}"#.utf8))
        #expect(report.restored.map(\.id) == ["A", "C"])
        #expect(report.failed.map(\.id) == ["D"])
        #expect(report.starting.map(\.id) == ["B"])
    }
    @Test func startingNoticeAndSummaryDoNotClaimReadiness() {
        L.$override.withValue(.en) {
            let result = RestoreResult(id: "A", name: "demo", ok: true, result: "starting")
            #expect(result.recoveryNotice == "demo is starting. Waiting for the session to confirm.")
            let summary = RestoreSummary(restored: 0, failed: 0, starting: 1)
            #expect(summary.title == "1 starting…")
            #expect(!summary.confirmed)
            let mixed = RestoreSummary(restored: 1, failed: 1, starting: 1)
            #expect(mixed.title == "Brought back 1 · 1 starting…")
            #expect(!mixed.confirmed)
        }
    }

    @Test func flowTracksAcceptedLaunchSeparatelyUntilSnapshotChanges() {
        var flow = RestoreFlow()
        let began = flow.begin(.one("A"))
        #expect(began)
        flow.finish(.one("A"), report: RestoreReport(results: [.init(id: "A", name: "demo", ok: true, result: "starting")]))
        #expect(flow.starting["A"]?.result == "starting")
        #expect(flow.failures.isEmpty)
        #expect(!flow.busy)
        flow.prune(keeping: ["A"])
        #expect(flow.starting["A"] != nil)
        flow.prune(keeping: [])
        #expect(flow.starting.isEmpty)
    }

    @Test func unknownAcceptedOutcomeDoesNotBecomeReadyAndFailureStillWins() {
        let future = RestoreResult(id: "A", name: "future", ok: true, result: "pending")
        #expect(!future.isRestored && future.isStarting)
        let failure = RestoreResult(id: "B", name: "failed", ok: false, result: "restored")
        #expect(!failure.isRestored && !failure.isStarting && failure.recoveryNotice == nil)
    }

}
