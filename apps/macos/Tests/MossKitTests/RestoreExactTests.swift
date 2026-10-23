import Foundation
import Testing
@testable import MossKit

private let uuidA = "11111111-1111-4111-8111-111111111111"
private let uuidB = "22222222-2222-4222-8222-222222222222"
private let uuidC = "33333333-3333-4333-8333-333333333333"
private let exact = CLICapabilities(workingDirectory: 1, newReceipt: 1, exactRestore: 1)

private func interrupted(_ slot: String, _ uuid: String, name: String? = "job", extra: [String: Any] = [:]) -> [String: Any] {
    with(sessionJSON(id: slot, name: name, status: "interrupted"), ["session_id": uuid, "working_directory": [
        "ok": true, "canonical": "/home/u/projects/acme", "reason": NSNull(), "message": NSNull(), "source_resolved": true,
    ] as [String: Any]].merging(extra) { _, new in new })
}

private func restored(_ slot: String, ok: Bool = true, reason: String? = nil) -> CommandResult {
    var row: [String: Any] = ["id": slot, "name": slot, "ok": ok]
    if let reason { row["reason"] = reason } else { row["result"] = "starting" }
    return CommandResult(status: 0, stdout: try! JSONSerialization.data(withJSONObject: ["schema": 1, "results": [row]]), stderr: "")
}

@Suite struct RestoreExactCapabilityTests {
    private func capabilities(_ json: String) throws -> CLICapabilities {
        try Snapshot.decode(Data(#"{"schema":1,"sessions":[],"capabilities":\#(json)}"#.utf8)).capabilities
    }

    @Test func onlyExactRestoreOneIsSupport() throws {
        #expect(try capabilities(#"{"working_directory":1,"new_receipt":1,"exact_restore":1}"#).restoresExactly)
        #expect(try !capabilities(#"{"working_directory":1,"new_receipt":1}"#).restoresExactly, "working_directory does not imply exact restore")
        #expect(try !capabilities(#"{"exact_restore":2}"#).restoresExactly)
        #expect(try !capabilities(#"{"exact_restore":"1"}"#).restoresExactly)
        #expect(!(try Snapshot.decode(snapshotData([])).capabilities.restoresExactly))
    }
}

@Suite struct RestoreExactArgumentTests {
    @Test func recordedRestoreNamesSlotAndFullUUID() {
        let session = sessions([interrupted("SLOT-A", uuidA)])[0]
        #expect(RestoreFlow.arguments(exact: RestoreTarget(session))
            == ["restore", "--yes", "--json", "--exact-slot-id", "SLOT-A", "--", uuidA])
    }

    @Test func unknownChoiceKeepsItsGuardAndIsExact() throws {
        let session = sessions([interrupted("SLOT-A", uuidA)])[0]
        for (source, tail) in [(RestoreConfigSource.defaultConfig, ["--config-source", "default"]),
                               (.directory("/tmp/settings dir"), ["--config-dir", "/tmp/settings dir"])] {
            var flow = RestoreSourceFlow()
            flow.offer(session, reason: .configSourceUnknown)
            let ticket = try #require(flow[session.id]?.ticket)
            let picked = flow.select(source, ticket: ticket, current: session, reason: .configSourceUnknown)
            #expect(picked)
            let pending = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
            let request = try #require(pending)
            #expect(request.arguments == ["restore", "--yes", "--json", "--exact-slot-id", "SLOT-A", "--require-unknown-config"] + tail + ["--", uuidA])
        }
    }

    @Test func knownReplacementKeepsItsRevisionGuardAndIsExact() throws {
        let session = sessions([interrupted("SLOT-A", uuidA, extra: ["restore_source": [
            "kind": "explicit", "status": "needs_location", "display_path": "/tmp/original", "default_path": "/Users/demo/.claude",
            "revision": "source-r1", "issue": "config_directory_missing",
        ] as [String: Any]])])[0]
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configDirectoryMissing)
        let ticket = try #require(flow[session.id]?.ticket)
        _ = flow.select(.directory("/tmp/replacement"), ticket: ticket, current: session, reason: .configDirectoryMissing)
        let pending = flow.confirm(ticket, current: session, reason: .configDirectoryMissing)
        let request = try #require(pending)
        #expect(request.arguments == ["restore", "--yes", "--json", "--exact-slot-id", "SLOT-A", "--expected-source-revision", "source-r1",
                                      "--config-dir", "/tmp/replacement", "--", uuidA])
        #expect(!request.arguments.contains("--require-unknown-config"))
    }

    @Test func aNameEqualToTheTargetUUIDCannotRedirectIt() {
        // Slot B is named exactly like A's conversation id; A's command names only A.
        let list = sessions([interrupted("SLOT-A", uuidA, name: "shared"), interrupted("SLOT-B", uuidB, name: uuidA)])
        let args = RestoreFlow.arguments(exact: RestoreTarget(list[0]))
        #expect(args == ["restore", "--yes", "--json", "--exact-slot-id", "SLOT-A", "--", uuidA])
        #expect(!args.contains("SLOT-B") && !args.contains("shared") && !args.contains(uuidB))
    }
}

@Suite struct RestoreExactClientTests {
    @Test func oldCLIGetsNoRestoreRequestAtAll() async throws {
        let session = sessions([interrupted("SLOT-A", uuidA)])[0]
        let runner = RecordingRunner([restored("SLOT-A")])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        let old = CLICapabilities(workingDirectory: 1, newReceipt: 1)
        await #expect(throws: MossClient.Failure.command(RestorePolicy.updateRequired)) {
            _ = try await client.restore(RestoreTarget(session), capabilities: old)
        }
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        _ = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        let pending = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        let request = try #require(pending)
        await #expect(throws: MossClient.Failure.command(RestorePolicy.updateRequired)) {
            _ = try await client.restore(request, capabilities: old)
        }
        await #expect(throws: MossClient.Failure.command(RestorePolicy.updateRequired)) {
            _ = try await client.restoreEach([RestoreTarget(session)], names: [:], capabilities: old) { _ in nil }
        }
        #expect(await runner.calls.isEmpty)
        L.$override.withValue(.zh) { #expect(RestorePolicy.updateRequired.contains("更新终端里的 cc-moss")) }
    }

    @Test func singleResultIsAttributedToTheTargetSlotOnly() async throws {
        let session = sessions([interrupted("SLOT-A", uuidA)])[0]
        let stray = CommandResult(status: 0, stdout: Data(#"{"schema":1,"results":[{"id":"SLOT-B","name":"other","ok":true}]}"#.utf8), stderr: "")
        let runner = RecordingRunner([stray])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner).restore(RestoreTarget(session), capabilities: exact)
        #expect(report.results.map(\.id) == ["SLOT-A"], "another slot's line is never taken as this one's")
        #expect(report.failed.first?.reason == .notFound)
        #expect(await runner.calls == [RestoreFlow.arguments(exact: RestoreTarget(session))])
    }
}

@Suite struct RestoreExactBatchTests {
    private let list = sessions([interrupted("SLOT-A", uuidA), interrupted("SLOT-B", uuidB), interrupted("SLOT-C", uuidC)])
    private var targets: [RestoreTarget] { list.map(RestoreTarget.init) }

    @Test func runsInOrderWithAtMostOneRequestEachAndNoResend() async throws {
        let failing = CommandResult(status: 1, stdout: Data(), stderr: "cc-moss: tmux is not installed\n")
        let runner = RecordingRunner([restored("SLOT-A"), failing, restored("SLOT-C", ok: false, reason: "tmux_failed")])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner)
            .restoreEach(targets, names: [:], capabilities: exact) { _ in nil }
        #expect(await runner.calls == targets.map(RestoreFlow.arguments(exact:)))
        #expect(report.results.map(\.id) == ["SLOT-A", "SLOT-B", "SLOT-C"])
        #expect(report.starting.map(\.id) == ["SLOT-A"])
        #expect(report.failed.map(\.id) == ["SLOT-B", "SLOT-C"])
        #expect(report.failed.first?.message == "cc-moss: tmux is not installed")
    }

    @Test func resultsBelongToTheirTargetNotToReturnOrder() async throws {
        // The CLI answers the second request with a line about another slot.
        let runner = RecordingRunner([restored("SLOT-A"), restored("SLOT-A"), restored("SLOT-C")])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner)
            .restoreEach(targets, names: ["SLOT-B": "job-b"], capabilities: exact) { _ in nil }
        #expect(report.results.map(\.id) == ["SLOT-A", "SLOT-B", "SLOT-C"])
        #expect(report.failed.map(\.id) == ["SLOT-B"], "an unattributed answer is not a success for B")
        #expect(report.failed.first?.name == "job-b")
        #expect(await runner.calls.count == 3)
    }

    @Test func aChangedGenerationIsSkippedNeverRetriedWithTheNewIdentity() async throws {
        var current = list
        current[1] = sessions([interrupted("SLOT-B", "44444444-4444-4444-8444-444444444444")])[0]
        let now = current
        let runner = RecordingRunner([restored("SLOT-A"), restored("SLOT-C")])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner)
            .restoreEach(targets, names: [:], capabilities: exact) { target in
                target.skip(current: now.first { $0.id == target.slotId }, capabilities: exact)
            }
        #expect(await runner.calls == [RestoreFlow.arguments(exact: targets[0]), RestoreFlow.arguments(exact: targets[2])])
        #expect(!(await runner.calls).joined().contains("44444444-4444-4444-8444-444444444444"))
        #expect(report.failed.map(\.id) == ["SLOT-B"])
        #expect(report.failed.first?.ok == false)
    }

    @Test func cancellationStopsBeforeTheNextRequest() async throws {
        let runner = RecordingRunner([restored("SLOT-A"), restored("SLOT-B"), restored("SLOT-C")])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        let report = try await client.restoreEach(targets, names: [:], capabilities: exact) { target in
            if target.slotId == "SLOT-B" { withUnsafeCurrentTask { $0?.cancel() } }
            return nil
        }
        #expect(await runner.calls == [RestoreFlow.arguments(exact: targets[0])])
        #expect(report.results.map(\.id) == ["SLOT-A"])
    }
}

@Suite struct RestoreTargetEligibilityTests {
    private let session = sessions([interrupted("SLOT-A", uuidA)])[0]

    @Test func currentInterruptedSessionIsEligible() {
        #expect(RestoreTarget(session).skip(current: session, capabilities: exact) == nil)
    }

    @Test func goneRenewedRunningOrArchivedSessionsAreSkipped() {
        let target = RestoreTarget(session)
        let renewed = sessions([interrupted("SLOT-A", uuidB)])[0]
        let running = sessions([with(interrupted("SLOT-A", uuidA), ["status": "running"])])[0]
        let archived = sessions([with(interrupted("SLOT-A", uuidA), ["archived_at": "2026-10-16T10:00:00Z"])])[0]
        for current in [nil, renewed, running, archived] {
            let skip = target.skip(current: current, capabilities: exact)
            #expect(skip?.ok == false)
            #expect(skip?.id == "SLOT-A")
        }
    }

    @Test func unknownSourceIsNeverAuthorisedByTheBatch() {
        let unknown = sessions([interrupted("SLOT-A", uuidA, extra: ["restore_source": [
            "kind": "unknown", "status": "needs_location", "display_path": NSNull(), "default_path": "/Users/demo/.claude",
            "revision": "r1", "issue": "config_source_unknown",
        ] as [String: Any]])])[0]
        #expect(RestoreTarget(unknown).skip(current: unknown, capabilities: exact)?.reason == .configSourceUnknown)
    }

    @Test func deniedWorkingDirectoryOrOldCLIIsSkipped() {
        let denied = sessions([interrupted("SLOT-A", uuidA, extra: ["working_directory": [
            "ok": false, "canonical": NSNull(), "reason": "cwd_forbidden", "message": NSNull(), "source_resolved": true,
        ] as [String: Any]])])[0]
        #expect(RestoreTarget(denied).skip(current: denied, capabilities: exact)?.reason == .cwdForbidden)
        #expect(RestoreTarget(session).skip(current: session, capabilities: .init(exactRestore: 1))?.ok == false)
    }

    @Test func lateFailureIsNotPaintedOnANewGeneration() {
        let target = RestoreTarget(session)
        let failure = RestoreResult(id: "SLOT-A", name: "job", ok: false, reason: .tmuxFailed)
        let report = RestoreReport(results: [failure])
        let renewed = sessions([interrupted("SLOT-A", uuidB)])[0]
        #expect(report.attributable(to: [target]) { _ in renewed }.results.isEmpty)
        #expect(report.attributable(to: [target]) { _ in session }.results == [failure])
        let started = RestoreReport(results: [RestoreResult(id: "SLOT-A", name: "job", ok: true, result: "starting")])
        #expect(started.attributable(to: [target]) { _ in renewed }.results.count == 1, "a started launch is still reported")
    }
}

@Suite struct RestoreExactChangedTests {
    private let session = sessions([interrupted("SLOT-A", uuidA)])[0]
    private let notFound = CommandResult(status: 0, stdout: Data(#"{"schema":1,"results":[{"id":"SLOT-A","name":"11111111-1111-4111-8111-111111111111","ok":false,"reason":"not_found","message":"the selected slot or current session identity changed; no session was started"}]}"#.utf8), stderr: "")
    private let empty = CommandResult(status: 0, stdout: Data(#"{"schema":1,"results":[]}"#.utf8), stderr: "")

    private func expectChanged(_ result: RestoreResult?) {
        guard let result else {
            Issue.record("the target has no failure line")
            return
        }
        #expect(result.id == "SLOT-A")
        #expect(!result.ok)
        #expect(!result.isRestored && !result.isStarting)
        #expect(result.reason == .notFound)
        L.$override.withValue(.zh) {
            let why = FailureNotice(restore: result, subject: nil).why
            #expect(why.contains("这条会话已变化，请刷新后再试"))
            #expect(!why.contains("删除") && !why.contains("不在"))
        }
    }

    @Test func notFoundIsAFailureSaidAsChanged() async throws {
        let runner = RecordingRunner([notFound])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner).restore(RestoreTarget(session), capabilities: exact)
        expectChanged(report.failed.first)
        #expect(report.results.count == 1)
        #expect(await runner.calls.count == 1, "never retried")
    }

    @Test func noLineForTheTargetIsAFailureNotASuccess() async throws {
        let runner = RecordingRunner([empty])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner).restore(RestoreTarget(session), capabilities: exact)
        expectChanged(report.failed.first)
        #expect(report.restored.isEmpty && report.starting.isEmpty)
        #expect(await runner.calls.count == 1)
    }

    @Test func sourceChoiceWithNoLineIsAFailure() async throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        _ = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        let pending = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        let request = try #require(pending)
        let runner = RecordingRunner([empty])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner).restore(request, capabilities: exact)
        expectChanged(report.failed.first)
    }

    @Test func batchChecksEveryTargetForItsOwnLine() async throws {
        let list = sessions([interrupted("SLOT-A", uuidA), interrupted("SLOT-B", uuidB)])
        let runner = RecordingRunner([notFound, empty])
        let report = try await MossClient(executable: "/x/cc-moss", runner: runner)
            .restoreEach(list.map(RestoreTarget.init), names: [:], capabilities: exact) { _ in nil }
        #expect(report.failed.map(\.id) == ["SLOT-A", "SLOT-B"])
        #expect(report.failed.allSatisfy { $0.reason == .notFound })
        #expect(await runner.calls.count == 2)
    }

    @Test func skippedRenewedTargetUsesTheSameWords() {
        let renewed = sessions([interrupted("SLOT-A", uuidB)])[0]
        expectChanged(RestoreTarget(session).skip(current: renewed, capabilities: exact))
    }
}

@Suite struct RestoreUpdateNoticeTests {
    @Test func oldCLINoticeIsTheAppsWordsNotACLIQuote() {
        L.$override.withValue(.zh) {
            let notice = FailureNotice(action: .restore, subject: nil, reason: RestorePolicy.updateRequiredCode, message: RestorePolicy.updateRequired)
            #expect(notice.why == RestorePolicy.updateRequired)
            #expect(!notice.why.hasPrefix("cc-moss 说"))
            #expect(notice.title == "没能找回会话", "a batch notice names no single session")
            let session = sessions([interrupted("SLOT-A", uuidA)])[0]
            let skipped = RestoreTarget(session).skip(current: session, capabilities: .init(workingDirectory: 1))
            #expect(skipped?.reasonCode == RestorePolicy.updateRequiredCode)
            #expect(FailureNotice(restore: skipped!, subject: nil).why == RestorePolicy.updateRequired)
        }
    }
}
