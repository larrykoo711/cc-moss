import Foundation
import Testing
@testable import MossKit

@Suite struct RestoreSourcePlanTests {
    private func planned(kind: String = "explicit", status: String = "needs_location", issue: String? = "config_directory_missing", revision: String = "source-r1") -> Session {
        sessions([with(sessionJSON(status: "interrupted"), ["restore_source": [
            "kind": kind, "status": status, "display_path": kind == "unknown" ? NSNull() : "/tmp/original",
            "default_path": "/Users/demo/.claude", "revision": revision, "issue": issue as Any? ?? NSNull()
        ] as [String: Any]])])[0]
    }

    @Test func oldOrMalformedPlanStaysUnknownInsteadOfGuessing() {
        let old = sessions([sessionJSON(status: "interrupted")])[0]
        let malformed = sessions([with(sessionJSON(status: "interrupted"), ["restore_source": "invalid"])])[0]
        #expect(old.restoreSource == nil)
        #expect(malformed.restoreSource == nil)
    }

    @Test func validRecordedSourceNeedsNoSourceChoiceOrOverride() {
        let session = planned(status: "ready", issue: nil)
        #expect(session.restoreSource?.isReady == true)
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: nil)
        #expect(flow[session.id] == nil)
        #expect(RestoreFlow.arguments(exact: RestoreTarget(session)) == ["restore", "--yes", "--json", "--exact-slot-id", session.id, "--", session.sessionId])
    }

    @Test func knownBrokenSourceReplacementUsesItsRevisionNotUnknownGuard() throws {
        let session = planned()
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configDirectoryMissing)
        let ticket = try #require(flow[session.id]?.ticket)
        let selected = flow.select(.directory("/tmp/replacement"), ticket: ticket, current: session, reason: .configDirectoryMissing)
        #expect(selected)
        let pending = flow.confirm(ticket, current: session, reason: .configDirectoryMissing)
        let request = try #require(pending)
        #expect(request.arguments == ["restore", "--yes", "--json", "--exact-slot-id", "01A", "--expected-source-revision", "source-r1", "--config-dir", "/tmp/replacement", "--", session.sessionId])
    }

    @Test func revisionChangeRejectsTheOldUnknownConfirmation() throws {
        let session = planned(kind: "unknown", issue: "config_source_unknown")
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        _ = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        var changed = session
        changed.restoreSource?.revision = "source-r2"
        let request = flow.confirm(ticket, current: changed, reason: .configSourceUnknown)
        #expect(request == nil)
    }

    @Test func knownSourceCannotUseAnOldUnknownFailureAsAuthority() {
        let session = planned(status: "ready", issue: nil)
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        #expect(flow[session.id] == nil)
    }

    @Test func unknownPlanSeparatesDefaultProposalFromRecordedLocation() {
        let session = planned(kind: "unknown", issue: "config_source_unknown")
        #expect(session.restoreSource?.displayPath == nil)
        #expect(session.restoreSource?.defaultPath == "/Users/demo/.claude")
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        #expect(flow[session.id]?.source == nil)
    }

    @Test func nonPathFailureDoesNotOfferLocationReplacement() {
        let session = planned(status: "ready", issue: nil)
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .exitedAtOnce)
        #expect(flow[session.id] == nil)
    }

    @Test func unavailablePlanWithdrawsOldConfirmationEvenWithSameRevision() throws {
        let session = planned()
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configDirectoryMissing)
        let ticket = try #require(flow[session.id]?.ticket)
        _ = flow.select(.directory("/tmp/new"), ticket: ticket, current: session, reason: .configDirectoryMissing)
        let blocked = planned(status: "blocked", issue: "config_source_unavailable")
        flow.observe([blocked])
        #expect(flow[session.id] == nil)
        let request = flow.confirm(ticket, current: blocked, reason: .configDirectoryMissing)
        #expect(request == nil)
    }

    @Test func missingRevisionDoesNotAuthorizeNewPlanConfirmation() {
        let session = planned(kind: "unknown", issue: "config_source_unknown", revision: "")
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        #expect(flow[session.id] == nil)
    }

    @Test func defaultProposalIsNotASelectionAndAdvancedModeRequiresOne() throws {
        let session = planned(kind: "unknown", issue: "config_source_unknown")
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let choice = try #require(flow[session.id])
        #expect(choice.offersDefaultProposal(for: session))
        #expect(choice.source == nil)
        flow.showAdvanced(choice.ticket, current: session, reason: .configSourceUnknown)
        #expect(flow[session.id]?.offersDefaultProposal(for: session) == false)
        #expect(flow[session.id]?.canConfirm == false)
    }

    @Test func unknownWithoutDefaultPathNeverInventsAProposalLocation() throws {
        var session = planned(kind: "unknown", issue: "config_source_unknown")
        session.restoreSource?.defaultPath = nil
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let choice = try #require(flow[session.id])
        #expect(!choice.offersDefaultProposal(for: session))
        #expect(choice.source == nil)
        let selected = flow.select(.defaultConfig, ticket: choice.ticket, current: session, reason: .configSourceUnknown)
        #expect(!selected)
        let request = flow.confirm(choice.ticket, current: session, reason: .configSourceUnknown)
        #expect(request == nil)
    }

    @Test func freshPathErrorPlanKeepsCorrectionButNotAnOldSelection() throws {
        let session = planned(status: "ready", issue: nil)
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configDirectoryMissing)
        let old = try #require(flow[session.id]?.ticket)
        let missing = planned(revision: "source-r2")
        flow.observe([missing])
        let current = try #require(flow[session.id])
        #expect(current.ticket != old)
        #expect(current.source == nil)
        #expect(current.issue == .configDirectoryMissing)
        _ = flow.select(.directory("/tmp/replacement"), ticket: current.ticket, current: missing, reason: .configDirectoryMissing)
        flow.observe([planned(revision: "source-r3")])
        #expect(flow[session.id] == nil)
    }

    @Test func unavailableIsNotAPathProblemAndNewPlanInvalidatesSearchCache() {
        #expect(!RestoreReason.configSourceUnavailable.isLocationProblem)
        let original = planned(status: "ready", issue: nil)
        let current = planned(status: "blocked", issue: "config_source_unavailable")
        let cache = SearchIndexes()
        _ = cache.live([original], folders: [:])
        #expect(cache.live([current], folders: [:]).search(current.title).first?.session == current)
    }

    @Test(arguments: ["config_directory_missing", "config_not_directory", "config_source_unreadable", "config_settings_invalid", "config_source_unavailable"])
    func sourceFailuresAreTypedRatherThanGeneric(_ code: String) throws {
        let report = try RestoreReport.decode(Data("{\"schema\":1,\"results\":[{\"id\":\"A\",\"name\":\"demo\",\"ok\":false,\"reason\":\"\(code)\"}]}".utf8))
        let failure = try #require(report.failed.first)
        #expect(failure.reason != .other)
        L.$override.withValue(.en) {
            #expect(!FailureNotice(restore: failure, subject: nil).why.contains("didn't say why"))
        }
    }
}
