import Foundation
import Testing
@testable import MossKit

@Suite struct RecoveryOperationTests {
    @Test func terminalFailuresExplainTheOutcomeWithoutSuggestingAnotherLaunch() {
        L.$override.withValue(.zh) {
            #expect(FailureNotice(action: .restart, subject: nil, reason: "quit-timeout").why == "旧会话还在运行，没有启动另一份。")
            #expect(FailureNotice(action: .restart, subject: nil, reason: "signal-failed").why == "无法请求旧会话退出，请打开会话检查。")
            #expect(FailureNotice(action: .restart, subject: nil, reason: "process-unknown").why == "无法确认进程状态，请打开会话检查。")
            #expect(FailureNotice(action: .restart, subject: nil, reason: "unsafe-terminal").why == "终端连接已变化，请打开会话检查。")
        }
    }
    private var session: Session { sessions([sessionJSON()])[0] }
    private func request(_ operations: inout RecoveryOperations, intent: RecoveryIntent = .restart) throws -> RecoveryRequest {
        let value = operations.begin(session, intent: intent)
        return try #require(value)
    }
    private func proxy() throws -> BatchResult {
        try BatchResult.decode(Data(#"{"schema":1,"results":[{"name":"demo","result":"refused","reason":"pty-proxy","rebuild_identity":{"slot_id":"01A","session_id":"01A-sid-0000","claude_pid":123,"claude_start":"a","tmux_socket":"/tmp/tmux-1/default","pane_id":"%1","pane_pid":100,"pane_start":"b"}}]}"#.utf8))[0]
    }

    @Test func oneSlotHasOneSubmittingStateAndRejectsDoubleClick() throws {
        var operations = RecoveryOperations()
        _ = try request(&operations)
        let duplicate = operations.begin(session, intent: .restart)
        #expect(duplicate == nil)
        #expect(operations.bySlot.count == 1)
        #expect(operations[session.id]?.isSubmitting == true)
        #expect(operations[session.id]?.progressLabel != nil)
    }

    @Test func trueBackendResultsAloneDecideStaticOutcome() throws {
        for result in ["starting", "queued", "restarted"] {
            var operations = RecoveryOperations()
            let started = try request(&operations)
            operations.complete(started, results: [.init(name: "demo", outcome: result)], session: session)
            #expect(operations[session.id]?.isSubmitting == false)
            #expect(operations[session.id]?.progressLabel == nil)
            let expected: RecoveryOperation.Stage = result == "starting" ? .starting : result == "queued" ? .queued : .restarted
            #expect(operations[session.id]?.stage == expected)
        }
    }

    @Test func failureEndsLoadingAndKeepsOneFoldableNotice() throws {
        for reason in ["quit-timeout", "signal-failed", "process-unknown", "unsafe-terminal", "confirmation-stale"] {
            var operations = RecoveryOperations()
            let started = try request(&operations)
            operations.complete(started, results: [.init(name: "demo", outcome: "refused", reason: reason, message: "technical detail")], session: session)
            #expect(operations[session.id]?.isFailure == true)
            #expect(operations[session.id]?.progressLabel == nil)
            #expect(operations[session.id]?.confirmation == nil)
            #expect(operations[session.id]?.failure?.message == "technical detail")
            #expect(operations.bySlot.count == 1)
            #expect(operations[session.id]?.explanation?.contains("technical detail") == false)
        }
    }

    @Test func confirmationIsAStageAndConsumedBeforeSubmittingRebuild() throws {
        var operations = RecoveryOperations()
        let started = try request(&operations)
        operations.complete(started, results: [try proxy()], session: session)
        #expect(operations[session.id]?.confirmation != nil)
        #expect(operations[session.id]?.progressLabel == nil)
        let value = operations.confirm(session)
        let rebuilding = try #require(value)
        #expect(operations[session.id]?.stage == .submitting(.rebuild))
        #expect(operations[session.id]?.confirmation == nil)
        var refused = try proxy()
        refused.reason = "unsafe-terminal"
        operations.complete(rebuilding, results: [refused], session: session)
        #expect(operations[session.id]?.isFailure == true)
        #expect(operations[session.id]?.confirmation == nil)
        let retried = operations.confirm(session)
        #expect(retried == nil)
    }

    @Test func cancellingConfirmationProducesNoRequest() throws {
        var operations = RecoveryOperations()
        let started = try request(&operations)
        operations.complete(started, results: [try proxy()], session: session)
        operations.cancelConfirmation(session.id)
        let next = operations.confirm(session)
        #expect(next == nil)
        #expect(operations[session.id] == nil)
    }

    @Test func lateAndDuplicateRepliesCannotOverwriteANewRequestOrRearmConsent() throws {
        var operations = RecoveryOperations()
        let old = try request(&operations)
        operations.complete(old, results: [try proxy()], session: session)
        operations.cancelConfirmation(session.id)
        let current = try request(&operations)
        let accepted = operations.complete(old, results: [try proxy()], session: session)
        #expect(!accepted)
        #expect(operations[session.id]?.token == current.token)
        operations.complete(current, results: [.init(name: "demo", outcome: "refused", reason: "quit-timeout")], session: session)
        let duplicate = operations.complete(current, results: [try proxy()], session: session)
        #expect(!duplicate)
        #expect(operations[session.id]?.isFailure == true)
        #expect(operations[session.id]?.confirmation == nil)
    }

    @Test func exceptionsEndLoadingWithoutLeavingConfirmation() throws {
        var operations = RecoveryOperations()
        let started = try request(&operations)
        operations.complete(started, results: [try proxy()], session: session)
        let value = operations.confirm(session)
        let rebuilding = try #require(value)
        operations.fail(rebuilding, notice: FailureNotice(action: .restart, subject: .init(session), error: MossClient.Failure.timedOut), session: session)
        #expect(operations[session.id]?.isFailure == true)
        #expect(operations[session.id]?.progressLabel == nil)
        #expect(operations[session.id]?.confirmation == nil)
    }

    @Test func queueDisappearingDoesNotMeanCancellationSucceeded() throws {
        var operations = RecoveryOperations()
        let started = try request(&operations)
        operations.complete(started, results: [.init(name: "demo", outcome: "queued")], session: session)
        let cancel = try request(&operations, intent: .cancelQueue)
        operations.complete(cancel, results: [.init(name: "demo", outcome: "refused", reason: "busy")], session: session)
        #expect(operations[session.id]?.isFailure == true)
        #expect(operations[session.id]?.stage != .cancelled)
        operations.dismiss(session.id)
        var queued = session
        queued.restartQueued = true
        let pending = operations.begin(queued, intent: .cancelQueue)
        let accepted = try #require(pending)
        operations.complete(accepted, results: [.init(name: "demo", outcome: "cancelled")], session: queued)
        #expect(operations[session.id]?.stage == .cancelled)
    }

    @Test func busyConfirmationFailsWithoutQueueOrIdentity() throws {
        var operations = RecoveryOperations()
        let started = try request(&operations)
        operations.complete(started, results: [try proxy()], session: session)
        var busy = session
        busy.busy = true
        let confirmed = operations.confirm(busy)
        #expect(confirmed == nil)
        #expect(operations[session.id]?.isFailure == true)
        #expect(operations[session.id]?.confirmation == nil)
        #expect(operations[session.id]?.stage != .queued)
    }

    @Test func unexpectedMissingResultsAreNotSuccessfulAndOtherSlotsStayIndependent() throws {
        var operations = RecoveryOperations()
        let first = try request(&operations)
        var other = session
        other.id = "other"
        other.sessionId = "other-session"
        let pending = operations.begin(other, intent: .restart)
        let second = try #require(pending)
        operations.complete(first, results: [], session: session)
        #expect(operations[session.id]?.isFailure == true)
        #expect(operations[other.id]?.token == second.token)
        #expect(operations[other.id]?.isSubmitting == true)
    }

    @Test func aRebuildCannotSilentlyBecomeQueuedOrAutomaticallyOfferAnotherIdentity() throws {
        for result in ["queued", "refused"] {
            var operations = RecoveryOperations()
            let first = try request(&operations)
            operations.complete(first, results: [try proxy()], session: session)
            let pending = operations.confirm(session)
            let second = try #require(pending)
            var response = try proxy()
            response.outcome = result
            operations.complete(second, results: [response], session: session)
            #expect(operations[session.id]?.isFailure == true)
            #expect(operations[session.id]?.confirmation == nil)
            #expect(operations[session.id]?.progressLabel == nil)
        }
    }

    @Test func snapshotCanRemoveAQueueButCannotInventACancellationOrRecoveryReceipt() throws {
        var operations = RecoveryOperations()
        let started = try request(&operations)
        operations.complete(started, results: [.init(name: "demo", outcome: "queued")], session: session)
        var queued = session
        queued.restartQueued = true
        operations.observe([queued])
        #expect(operations[session.id]?.stage == .queued)
        operations.observe([session])
        #expect(operations[session.id] == nil)
        let another = try request(&operations)
        operations.complete(another, results: [.init(name: "demo", outcome: "starting")], session: session)
        operations.observe([session])
        #expect(operations[session.id]?.stage == .starting)
    }

    @Test func queuedWithoutBusyEvidenceIsNeutralAndInProgressKeepsItsReason() throws {
        var operations = RecoveryOperations()
        let first = try request(&operations)
        operations.complete(first, results: [.init(name: "demo", outcome: "queued")], session: session)
        #expect(operations[session.id]?.explanation == nil)
        operations.dismiss(session.id)
        let second = try request(&operations)
        operations.complete(second, results: [.init(name: "demo", outcome: "queued", reason: "in-progress")], session: session)
        L.$override.withValue(.zh) {
            #expect(operations[session.id]?.explanation == "正在等待另一项恢复操作。")
        }
    }

    @Test func cancelNotQueuedIsNeutralRatherThanACancellationReceipt() throws {
        var operations = RecoveryOperations()
        var queued = session
        queued.restartQueued = true
        let value = operations.begin(queued, intent: .cancelQueue)
        let cancel = try #require(value)
        operations.complete(cancel, results: [.init(name: "demo", outcome: "cancelled", reason: "not-queued", message: "当前没有排队操作")], session: queued)
        L.$override.withValue(.zh) {
            #expect(operations[session.id]?.title == "当前没有排队操作")
        }
        #expect(operations[session.id]?.stage != .cancelled)
    }

    @Test func copyDetailsContainsUnknownMessageOnce() {
        let message = "unique diagnostic detail"
        for language in [Lang.zh, Lang.en] {
            L.$override.withValue(language) {
                for action in [FailureNotice.Action.restart, .send, .archive] {
                    let notice = FailureNotice(action: action, subject: nil, reason: "new-reason", message: message)
                    #expect(notice.details.components(separatedBy: message).count - 1 == 1)
                }
            }
        }
    }

    @Test func queuedReasonIsPreservedAndANewerIdleSessionSuppressesBusyWording() throws {
        for reason in ["busy", "in-progress", "future-reason"] {
            var operations = RecoveryOperations()
            let first = try request(&operations)
            operations.complete(first, results: [.init(name: "demo", outcome: "queued", reason: reason)], session: session)
            let operation = try #require(operations[session.id])
            #expect(operation.queueReason == .init(reason))
            L.$override.withValue(.zh) {
                if reason == "busy" {
                    #expect(operation.explanation(currentBusy: true) == "本轮结束后执行。")
                    #expect(operation.explanation(currentBusy: false) == nil)
                } else if reason == "in-progress" {
                    #expect(operation.explanation(currentBusy: false) == "正在等待另一项恢复操作。")
                } else {
                    #expect(operation.explanation == nil)
                }
            }
        }
    }

    @Test func executingCancellationIsRefusedRatherThanClaimingCancellation() throws {
        var operations = RecoveryOperations()
        var queued = session
        queued.restartQueued = true
        let pending = operations.begin(queued, intent: .cancelQueue)
        let request = try #require(pending)
        operations.complete(request, results: [.init(name: "demo", outcome: "refused", reason: "in-progress")], session: queued)
        #expect(operations[session.id]?.isFailure == true)
        #expect(operations[session.id]?.stage != .cancelled)
        L.$override.withValue(.zh) {
            #expect(operations[session.id]?.explanation == "恢复操作已经开始，请打开会话检查。")
        }
    }

    @Test func typedWhyAndRawDiagnosticAreBothPreservedWithoutDuplication() {
        L.$override.withValue(.zh) {
            let notice = FailureNotice(action: .restart, subject: nil, reason: "quit-timeout", message: "process still alive")
            #expect(notice.details.contains("旧会话还在运行，没有启动另一份。"))
            #expect(notice.details.components(separatedBy: "process still alive").count - 1 == 1)
            let same = FailureNotice(action: .restart, subject: nil, reason: "quit-timeout", message: notice.why)
            #expect(same.details.components(separatedBy: notice.why).count - 1 == 1)
        }
    }

    @Test func startingCannotConvergeFromRunningAloneButCanBeDismissedWithoutRetry() throws {
        var operations = RecoveryOperations()
        let pending = try request(&operations)
        operations.complete(pending, results: [.init(name: "demo", outcome: "starting")], session: session)
        var running = session
        running.lastActiveAt = Date()
        operations.observe([running])
        #expect(operations[session.id]?.stage == .starting)
        #expect(operations[session.id]?.progressLabel == nil)
        operations.dismiss(session.id)
        #expect(operations[session.id] == nil)
    }

    @Test func restoredReceiptClearsOnceTheSessionIsUsedAgain() throws {
        for used in [true, false] {
            var operations = RecoveryOperations()
            let pending = try request(&operations)
            operations.complete(pending, results: [.init(name: "demo", outcome: "restarted")], session: session)
            var later = session
            later.lastActiveAt = Date()
            operations.observe([later])
            #expect(operations[session.id]?.stage == .restarted)
            if used { later.busy = true } else { later.lastPrompt = "next question" }
            operations.observe([later])
            #expect(operations[session.id] == nil)
        }
    }

    @Test func startingStaysUntilDismissedEvenWhenTheSessionWorks() throws {
        var operations = RecoveryOperations()
        let pending = try request(&operations)
        operations.complete(pending, results: [.init(name: "demo", outcome: "starting")], session: session)
        var busy = session
        busy.busy = true
        operations.observe([busy])
        #expect(operations[session.id]?.stage == .starting)
    }

    @Test func replacementSessionRejectsOldSuccessAndInvalidatesState() throws {
        var operations = RecoveryOperations()
        let old = try request(&operations)
        var replacement = session
        replacement.sessionId = "replacement-session"
        operations.observe([replacement])
        #expect(operations[session.id] == nil)
        let accepted = operations.complete(old, results: [.init(name: "old", outcome: "restarted")], session: replacement)
        #expect(!accepted)
        #expect(operations[session.id] == nil)
    }

    @Test func replacementSessionRejectsOldFailureAndInvalidatesState() throws {
        var operations = RecoveryOperations()
        let old = try request(&operations)
        var replacement = session
        replacement.sessionId = "replacement-session"
        operations.observe([replacement])
        let accepted = operations.fail(old, notice: FailureNotice(action: .restart, subject: nil, reason: "quit-timeout"), session: replacement)
        #expect(!accepted)
        #expect(operations[session.id] == nil)
    }

    @Test func removedSessionRejectsBothOldSuccessAndOldFailure() throws {
        for failure in [false, true] {
            var operations = RecoveryOperations()
            let old = try request(&operations)
            operations.observe([])
            #expect(operations[session.id] == nil)
            let accepted: Bool
            if failure {
                accepted = operations.fail(old, notice: FailureNotice(action: .restart, subject: nil, reason: "timeout"), session: nil)
            } else {
                accepted = operations.complete(old, results: [.init(name: "old", outcome: "restarted")], session: session)
            }
            #expect(!accepted)
            #expect(operations[session.id] == nil)
        }
    }

    @Test func completionChecksCurrentIdentityEvenBeforeObserveRuns() throws {
        var operations = RecoveryOperations()
        let old = try request(&operations)
        var replacement = session
        replacement.sessionId = "replacement-session"
        let accepted = operations.complete(old, results: [.init(name: "old", outcome: "restarted")], session: replacement)
        #expect(!accepted)
        #expect(operations[session.id] == nil)
    }

    @Test func failureChecksCurrentIdentityEvenBeforeObserveRuns() throws {
        for removed in [false, true] {
            var operations = RecoveryOperations()
            let old = try request(&operations)
            var replacement = session
            replacement.sessionId = "replacement-session"
            let accepted = operations.fail(old, notice: FailureNotice(action: .restart, subject: nil, reason: "timeout"),
                                           session: removed ? nil : replacement)
            #expect(!accepted)
            #expect(operations[session.id] == nil)
        }
    }

    @Test func oldRepliesCannotEraseOrContaminateReplacementRequests() throws {
        var operations = RecoveryOperations()
        let old = try request(&operations)
        var replacement = session
        replacement.sessionId = "replacement-session"
        let value = operations.begin(replacement, intent: .restart)
        let current = try #require(value)
        #expect(current.sessionId == replacement.sessionId)
        #expect(operations[session.id]?.sessionId == replacement.sessionId)
        let success = operations.complete(old, results: [.init(name: "old", outcome: "restarted")], session: replacement)
        let failure = operations.fail(old, notice: FailureNotice(action: .restart, subject: nil, reason: "quit-timeout"), session: replacement)
        #expect(!success && !failure)
        #expect(operations[session.id]?.token == current.token)
        #expect(operations[session.id]?.isSubmitting == true)
        let accepted = operations.complete(current, results: [.init(name: "current", outcome: "restarted")], session: replacement)
        #expect(accepted)
        #expect(operations[session.id]?.stage == .restarted)
    }

    @Test func removalBeforeObserveRejectsSuccessAndSnapshotInvalidatesConfirmation() throws {
        var operations = RecoveryOperations()
        let first = try request(&operations)
        let accepted = operations.complete(first, results: [.init(name: "old", outcome: "restarted")], session: nil)
        #expect(!accepted)
        #expect(operations[session.id] == nil)
        let second = try request(&operations)
        operations.complete(second, results: [try proxy()], session: session)
        operations.observe([])
        #expect(operations[session.id] == nil)
        let confirmed = operations.confirm(session)
        #expect(confirmed == nil)
    }

}
