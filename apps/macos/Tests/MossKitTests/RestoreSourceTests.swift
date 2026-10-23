import Foundation
import Testing
@testable import MossKit

@Suite struct RestoreSourceTests {
    private var session: Session { sessions([sessionJSON(status: "interrupted")])[0] }

    @Test func onlyUnknownFailuresOfferAnUnselectedChoice() {
        for reason in [RestoreReason.configSourceUnsupported, .configSourceChanged, .invalidConfigSource, .resumePreflightFailed, .tmuxFailed] {
            var flow = RestoreSourceFlow()
            flow.offer(session, reason: reason)
            #expect(flow[session.id] == nil)
        }
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        #expect(flow[session.id]?.source == nil)
        #expect(flow[session.id] != nil)
    }

    @Test func defaultIsOneExplicitSingleUseRequest() throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        let unselected = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        #expect(unselected == nil)
        let selected = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        #expect(selected)
        let pending = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        let request = try #require(pending)
        #expect(request.arguments == ["restore", "--yes", "--json", "--exact-slot-id", "01A", "--require-unknown-config", "--config-source", "default", "--", session.sessionId])
        let repeated = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        #expect(repeated == nil)
    }

    @Test func cancelRejectsLatePickerAndCannotLaunch() throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let pending = flow.beginPicker(current: session, reason: .configSourceUnknown)
        let ticket = try #require(pending)
        flow.cancel(session.id)
        let received = flow.receiveDirectory("/tmp/config", ticket: ticket, current: session, reason: .configSourceUnknown)
        #expect(!received)
        let request = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        #expect(request == nil)
    }

    @Test func reopeningSameSessionRejectsTheOldPickerToken() throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let firstPending = flow.beginPicker(current: session, reason: .configSourceUnknown)
        let first = try #require(firstPending)
        flow.cancel(session.id)
        flow.offer(session, reason: .configSourceUnknown)
        let secondPending = flow.beginPicker(current: session, reason: .configSourceUnknown)
        let second = try #require(secondPending)
        #expect(first != second)
        let stale = flow.receiveDirectory("/tmp/old", ticket: first, current: session, reason: .configSourceUnknown)
        #expect(!stale)
        let fresh = flow.receiveDirectory("/tmp/new", ticket: second, current: session, reason: .configSourceUnknown)
        #expect(fresh)
        #expect(flow[session.id]?.source == .directory("/tmp/new"))
    }

    @Test func pickerCancellationClearsChoiceAndInvalidatesItsToken() throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let pending = flow.beginPicker(current: session, reason: .configSourceUnknown)
        let ticket = try #require(pending)
        flow.pickerCancelled(ticket)
        #expect(flow[session.id] == nil)
        let late = flow.receiveDirectory("/tmp/late", ticket: ticket, current: session, reason: .configSourceUnknown)
        #expect(!late)
        #expect(flow.picker == nil)
    }

    @Test func onePickerAcrossDifferentRows() throws {
        var flow = RestoreSourceFlow()
        var second = session
        second.id = "second"
        second.sessionId = "second-session"
        flow.offer(session, reason: .configSourceUnknown)
        flow.offer(second, reason: .configSourceUnknown)
        let first = flow.beginPicker(current: session, reason: .configSourceUnknown)
        #expect(first != nil)
        let repeated = flow.beginPicker(current: second, reason: .configSourceUnknown)
        #expect(repeated == nil)
        #expect(flow[second.id]?.source == nil)
    }

    @Test func replacementDeletionAndReasonChangesRefuseConfirmation() throws {
        for replacement in [true, false] {
            var flow = RestoreSourceFlow()
            flow.offer(session, reason: .configSourceUnknown)
            let ticket = try #require(flow[session.id]?.ticket)
            let selected = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
            #expect(selected)
            var newer = session
            newer.sessionId = "new-generation"
            let request = flow.confirm(ticket, current: replacement ? newer : nil, reason: .configSourceUnknown)
            #expect(request == nil)
        }
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        let selected = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        #expect(selected)
        let request = flow.confirm(ticket, current: session, reason: .configSourceUnsupported)
        #expect(request == nil)
    }

    @Test func selectedDirectoryIsAnIndependentArgumentAndRequiresConfirmation() async throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let pending = flow.beginPicker(current: session, reason: .configSourceUnknown)
        let ticket = try #require(pending)
        let path = "/tmp/config with spaces;$(not-a-command)"
        let received = flow.receiveDirectory(path, ticket: ticket, current: session, reason: .configSourceUnknown)
        #expect(received)
        #expect(flow[session.id]?.submitting == false)
        let confirmed = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        let request = try #require(confirmed)
        #expect(request.arguments == ["restore", "--yes", "--json", "--exact-slot-id", "01A", "--require-unknown-config", "--config-dir", path, "--", session.sessionId])
        let runner = Recorder(stdout: "{\"schema\":1,\"results\":[]}")
        _ = try await MossClient(executable: "/demo/cc-moss", runner: runner).restore(request, capabilities: .init(workingDirectory: 1, exactRestore: 1))
        #expect(await runner.calls == [request.arguments])
    }

    @Test func invalidDirectorySyntaxIsNotSelectable() throws {
        for path in ["relative", "", "/tmp/bad\nname", "/tmp/bad\u{0}name"] {
            var flow = RestoreSourceFlow()
            flow.offer(session, reason: .configSourceUnknown)
            let pending = flow.beginPicker(current: session, reason: .configSourceUnknown)
            let ticket = try #require(pending)
            let received = flow.receiveDirectory(path, ticket: ticket, current: session, reason: .configSourceUnknown)
            #expect(!received)
        }
    }

    @Test func snapshotReplacementOrDeletionInvalidatesPicker() throws {
        for removed in [true, false] {
            var flow = RestoreSourceFlow()
            flow.offer(session, reason: .configSourceUnknown)
            let pending = flow.beginPicker(current: session, reason: .configSourceUnknown)
            let ticket = try #require(pending)
            var replacement = session
            replacement.sessionId = "replacement-session"
            flow.observe(removed ? [] : [replacement])
            #expect(flow.picker == nil)
            let received = flow.receiveDirectory("/tmp/config", ticket: ticket, current: removed ? nil : replacement, reason: .configSourceUnknown)
            #expect(!received)
            #expect(flow[session.id] == nil)
        }
    }

    @Test func aNewFailureOrRunningStateWithdrawsTheChoice() throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        flow.offer(session, reason: .configSourceUnsupported)
        #expect(flow[session.id] == nil)
        flow.offer(session, reason: .configSourceUnknown)
        var running = session
        running.status = "running"
        flow.observe([running])
        #expect(flow[session.id] == nil)
    }

    @Test func realTmuxFailureKeepsItsOwnExplanation() {
        L.$override.withValue(.en) {
            let notice = FailureNotice(action: .restore, subject: nil, reason: "tmux_failed")
            #expect(notice.why.contains("tmux"))
            #expect(RestoreReason.tmuxFailed.explanation.contains("tmux"))
        }
    }

    @Test func sourceChangedClearsConfirmationWithoutRetrying() async throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        _ = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        let pending = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        let request = try #require(pending)
        let runner = Recorder(stdout: "{\"schema\":1,\"results\":[{\"id\":\"01A\",\"name\":\"demo\",\"ok\":false,\"reason\":\"config_source_changed\"}]}")
        let report = try await MossClient(executable: "/demo/cc-moss", runner: runner).restore(request, capabilities: .init(workingDirectory: 1, exactRestore: 1))
        let failure = try #require(report.failed.first)
        flow.cancel(ticket.slotId)
        flow.offer(session, reason: failure.reason)
        #expect(flow[session.id] == nil)
        let retry = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        #expect(retry == nil)
        #expect(await runner.calls == [["restore", "--yes", "--json", "--exact-slot-id", "01A", "--require-unknown-config", "--config-source", "default", "--", session.sessionId]])
        #expect(failure.reason != .other)
    }

    @Test(arguments: ["config_source_unknown", "config_source_unsupported", "invalid_config_source", "resume_preflight_failed", "config_source_changed"])
    func typedSourceFailuresDoNotFallBackToGenericOrTmux(_ code: String) throws {
        let data = Data("{\"schema\":1,\"results\":[{\"id\":\"A\",\"name\":\"demo\",\"ok\":false,\"reason\":\"\(code)\"}]}".utf8)
        let result = try #require(RestoreReport.decode(data).results.first)
        #expect(result.reason != .other)
        #expect(result.reason != .tmuxFailed)
        L.$override.withValue(.en) {
            let notice = FailureNotice(restore: result, subject: nil)
            #expect(!notice.why.contains("didn't say why"))
            #expect(!notice.why.contains("tmux"))
        }
    }
}
