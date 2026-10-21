import Foundation
import Testing
@testable import MossKit

@Suite struct ProviderRecoveryTests {
    @Test func ordinaryRestartDoesNotNeedProviderBaseline() {
        let s = sessions([sessionJSON()])[0]
        #expect(Restart.allowed(s))
        #expect(RowActions(s, hopeless: false).more.joined().contains(.restart))
        #expect(!RowActions(s, hopeless: false).inline.contains(.restart))
    }

    @Test func flaggedRestartIsNotAnIsolatedInlineIcon() {
        let s = sessions([with(sessionJSON(), ["provider_restart": "model"])])[0]
        #expect(!RowActions(s, hopeless: false).inline.contains(.restart))
    }

    @Test func batchApplicationExplicitlySelectsProviderDefault() {
        #expect(Restart.staleArguments.contains("--provider-default"))
    }

    @Test func recoveryNeverClaimsAnAPIWasVerified() {
        L.$override.withValue(.en) {
            #expect(Restart.summary([BatchResult(name: "demo", outcome: "restarted")]) == "demo's session was restored.")
            #expect(Restart.summary([BatchResult(name: "demo", outcome: "starting")]) == "demo is restarting…")
            #expect(Restart.summary([BatchResult(name: "demo", outcome: "cancelled")]) == "demo's queued restart was cancelled.")
        }
    }
    private func planned(revision: String = "revision-a", model: Any = "model-b", uncertain: Bool = true,
                         extras: [String: Any] = [:]) -> Session {
        sessions([with(sessionJSON(), ["provider_model": ["model": model, "source": "settings", "revision": revision,
                                                         "uncertain": uncertain]].merging(extras) { _, new in new })])[0]
    }

    @Test func oldCLIAndMalformedOptionalFieldsKeepTheSession() {
        let old = sessions([sessionJSON()])[0]
        #expect(old.providerModel == nil && old.apiFailure == nil)
        #expect(ProviderRecovery.state(old) == nil)
        #expect(!ProviderRecovery.canApply(old))
        let malformed = sessions([with(sessionJSON(), ["provider_model": "bad", "api_failure": 123])])[0]
        #expect(malformed.providerModel == nil && malformed.apiFailure == nil)
    }

    @Test func resolvedTargetCanApplyEvenWhenCurrentProcessIsUncertain() {
        let s = planned()
        #expect(s.providerModel?.model == "model-b")
        #expect(s.providerModel?.source == "settings")
        #expect(ProviderRecovery.state(s) == .uncertain)
        #expect(ProviderRecovery.canApply(s))
        #expect(!ProviderRecovery.canApply(planned(model: NSNull())))
    }

    @Test func confirmationOnlyHidesThisSlotsRevision() {
        let s = planned(extras: ["provider_restart": "model"])
        var confirmations = ProviderConfirmations()
        confirmations.confirm(s)
        #expect(ProviderRecovery.state(s, confirmations: confirmations) == nil)
        #expect(ProviderRecovery.state(planned(revision: "revision-b"), confirmations: confirmations) == .uncertain)
        var other = s
        other.id = "other"
        #expect(ProviderRecovery.state(other, confirmations: confirmations) == .changed)
        other = s
        other.providerRestart = .removed
        #expect(ProviderRecovery.state(other, confirmations: confirmations) == .changed)
    }

    @Test func failureSurvivesConfirmationAndSameModelIsNotPromoted() {
        let s = planned(extras: ["api_failure": ["kind": "model_unavailable", "status": 400, "model": "model-b", "at_ms": 123]])
        var confirmations = ProviderConfirmations()
        confirmations.confirm(s)
        #expect(s.apiFailure?.atMs == 123)
        #expect(ProviderRecovery.state(s, confirmations: confirmations) == .modelUnavailable)
        #expect(!ProviderRecovery.canApply(s))
        let other = planned(extras: ["api_failure": ["kind": "model_unavailable", "model": "model-a"]])
        #expect(ProviderRecovery.canApply(other))
        #expect(s.apiFailure?.status == 400)
    }

    @Test func busyQueuedAndProtectedSessions() {
        var s = planned(extras: ["busy": true])
        #expect(ProviderRecovery.canApply(s))
        s.restartQueued = true
        #expect(ProviderRecovery.state(s) == .queued)
        #expect(!ProviderRecovery.canApply(s))
        s.restartQueued = false
        s.role = "moss"
        #expect(!Restart.allowed(s) && ProviderRecovery.state(s) == nil)
        s.role = nil
        s.archivedAt = Date()
        #expect(!Restart.allowed(s) && ProviderRecovery.state(s) == nil)
        s.archivedAt = nil
        s.status = "interrupted"
        #expect(!Restart.allowed(s) && ProviderRecovery.state(s) == nil)
        s.status = "running"
        s.tmux = nil
        #expect(!Restart.allowed(s))
    }

    @Test func queuedOnlyBannerNeverClaimsALiveSwitch() {
        let s = planned(extras: ["restart_queued": true, "provider_restart": "model"])
        let now = Date()
        var banner = ProviderBanner()
        banner.take([SessionEvent(seq: 1, slotId: "", kind: ProviderChange.kind, at: now,
                                  provider: ProviderChange(host: nil, live: false))], now: now)
        #expect(banner.state(now: now, sessions: [s]) == .queued(count: 1))
    }

    @Test func applyCancelAndOrdinaryRestartHaveDistinctArguments() async throws {
        let runner = Recorder(stdout: #"{"schema":1,"results":[{"name":"demo","result":"starting"}]}"#)
        let client = MossClient(executable: "/demo/cc-moss", runner: runner)
        _ = try await client.restart(["-demo"])
        _ = try await client.applyProvider(["-demo"])
        _ = try await client.cancelRestart(["-demo"])
        #expect(await runner.calls == [["restart", "--json", "--", "-demo"],
                                       ["restart", "--provider-default", "--json", "--", "-demo"],
                                       ["restart", "--cancel", "--json", "--", "-demo"]])
    }

    @Test func refusalDetailsReachTheExistingFailureNotice() throws {
        let result = try BatchResult.decode(Data(#"{"schema":1,"results":[{"name":"demo","result":"refused","reason":"model-unresolved","message":"Select a model first"}]}"#.utf8))[0]
        #expect(result.message == "Select a model first")
        #expect(Restart.refusal(result) == "Select a model first")
        #expect(FailureNotice(action: .restart, batch: result, subject: nil).message == "Select a model first")
    }

    @Test func persistenceUsesOnlyTheInjectedDefaults() throws {
        let suite = "cc-moss-test-provider-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        var confirmations = ProviderConfirmations()
        confirmations.confirm(planned())
        confirmations.save(defaults)
        #expect(ProviderConfirmations.load(defaults) == confirmations)
        #expect(defaults.dictionaryRepresentation()["provider_started"] == nil)
    }

    @Test func bulkApplicationCannotOverrideKeptSelectionOrRetryTheSameFailedModel() {
        let s = planned(extras: ["provider_restart": "model"])
        #expect(ProviderRecovery.canApplyAll([s], confirmations: .init()))
        var confirmations = ProviderConfirmations()
        confirmations.confirm(s)
        #expect(!ProviderRecovery.canApplyAll([s], confirmations: confirmations))
        let failed = planned(extras: ["provider_restart": "model", "api_failure": ["kind": "generic_api", "model": "model-b"]])
        #expect(!ProviderRecovery.canApplyAll([failed], confirmations: .init()))
        #expect(!ProviderRecovery.canApplyAll([], confirmations: .init()))
    }

    @Test func keepSelectionIsOnlyOfferedForNonFailureModelOrUnknownPrompts() {
        #expect(ProviderRecovery.canKeepSelection(planned(extras: ["provider_restart": "model"])))
        #expect(ProviderRecovery.canKeepSelection(planned(model: NSNull())))
        #expect(!ProviderRecovery.canKeepSelection(planned(extras: ["api_failure": ["kind": "model_unavailable", "model": "model-a"]])))
        #expect(!ProviderRecovery.canKeepSelection(planned(extras: ["api_failure": ["kind": "generic_api"]])))
        #expect(!ProviderRecovery.canKeepSelection(planned(extras: ["provider_restart": "removed"])))
        #expect(!ProviderRecovery.canKeepSelection(planned(extras: ["restart_queued": true])))
        #expect(!ProviderRecovery.canKeepSelection(planned(uncertain: false)))
        #expect(!ProviderRecovery.canKeepSelection(planned(revision: "")))
    }

}
