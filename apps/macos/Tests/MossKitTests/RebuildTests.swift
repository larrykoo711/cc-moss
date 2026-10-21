import Foundation
import Testing
@testable import MossKit

@Suite struct RebuildTests {
    @Test func staleConfirmationRequiresASeparateNewConfirmation() {
        L.$override.withValue(.zh) {
            #expect(Restart.refusal(.init(name: "demo", outcome: "refused", reason: "confirmation-stale"))
                    == "窗格身份已变化，请重新点击重开并确认。")
            #expect(Restart.refusal(.init(name: "demo", outcome: "refused", reason: "confirmation-stale", message: "PID changed"))
                    == "窗格身份已变化，请重新点击重开并确认。")
        }
    }

    @Test func busyRebuildNeverPromisesAQueuedAction() {
        L.$override.withValue(.zh) {
            #expect(Restart.refusal(.init(name: "demo", outcome: "refused", reason: "busy"))
                    == "会话正在忙，本轮结束后再重建。")
        }
    }
    private var session: Session { sessions([sessionJSON()])[0] }
    private var identity: [String: Any] {
        ["slot_id": "01A", "session_id": "01A-sid-0000", "claude_pid": 123,
         "claude_start": "start-a", "tmux_socket": "/tmp/tmux-1/default", "pane_id": "%1",
         "pane_pid": 100, "pane_start": "start-b"]
    }

    private func refusal(identity: [String: Any]?, reason: String = "pty-proxy") throws -> BatchResult {
        var result: [String: Any] = ["name": "demo", "result": "refused", "reason": reason]
        result["rebuild_identity"] = identity
        return try BatchResult.decode(JSONSerialization.data(withJSONObject: ["schema": 1, "results": [result]]))[0]
    }

    @Test func oldJSONAndIncompleteIdentitiesNeverOfferRebuild() throws {
        var gate = RebuildGate()
        let old = try refusal(identity: nil)
        #expect(old.rebuildIdentity == nil)
        let check1 = !gate.offer(old, session: session, applyingProvider: false)
        #expect(check1)
        for key in identity.keys {
            var missing = identity
            missing.removeValue(forKey: key)
            let result = try refusal(identity: missing)
            let check2 = !gate.offer(result, session: session, applyingProvider: false)
            #expect(check2)
            var empty = identity
            empty[key] = key.hasSuffix("_pid") ? 0 : ""
            let bad = try refusal(identity: empty)
            let check3 = !gate.offer(bad, session: session, applyingProvider: false)
            #expect(check3)
        }
    }

    @Test func confirmationReturnsExactIdentityAndOriginalProviderPolicy() async throws {
        for provider in [false, true] {
            var gate = RebuildGate()
            let result = try refusal(identity: identity)
            let check4 = gate.offer(result, session: session, applyingProvider: provider)
            #expect(check4)
            let confirmed = gate.confirm(session)
            let plan = try #require(confirmed)
            #expect(gate.pending == nil)
            let args = try plan.arguments()
            #expect(Array(args.prefix(4)) == ["restart", "--rebuild-pane", "--yes", "--rebuild-identity"])
            let returned = try JSONSerialization.jsonObject(with: Data(args[4].utf8)) as? NSDictionary
            #expect(returned == identity as NSDictionary)
            #expect(args.contains("--provider-default") == provider)
            #expect(Array(args.suffix(3)) == ["--json", "--", session.target])
            #expect(!args.contains("--stale") && !args.contains("--now") && !args.contains("--cancel"))
            let runner = Recorder(stdout: #"{"schema":1,"results":[{"name":"demo","result":"starting"}]}"#)
            let client = MossClient(executable: "/demo/cc-moss", runner: runner)
            _ = try await client.rebuildPane(plan)
            #expect(await runner.calls == [args])
            let check5 = gate.confirm(session) == nil
            #expect(check5)
        }
    }

    @Test func cancellingSendsNoCommand() async throws {
        var gate = RebuildGate()
        let check6 = gate.offer(try refusal(identity: identity), session: session, applyingProvider: true)
        #expect(check6)
        gate.cancel()
        let runner = Recorder(stdout: #"{"schema":1,"results":[]}"#)
        let client = MossClient(executable: "/demo/cc-moss", runner: runner)
        if let plan = gate.confirm(session) { _ = try await client.rebuildPane(plan) }
        #expect(await runner.calls.isEmpty)
        #expect(gate.pending == nil)
    }

    @Test func busyOrChangedSessionCannotConsumeConsentAsACommand() throws {
        for change in ["busy", "session", "pane", "socket", "moss", "archived", "queued", "status"] {
            var gate = RebuildGate()
            let check7 = gate.offer(try refusal(identity: identity), session: session, applyingProvider: false)
            #expect(check7)
            var current = session
            switch change {
            case "busy": current.busy = true
            case "session": current.sessionId = "another-session"
            case "pane": current.tmux?.paneId = "%2"
            case "socket": current.tmux?.socket = "/tmp/another.sock"
            case "moss": current.role = "moss"
            case "archived": current.archivedAt = Date()
            case "queued": current.restartQueued = true
            default: current.status = "interrupted"
            }
            #expect(!gate.canConfirm(current))
            let check8 = gate.confirm(current) == nil
            #expect(check8)
            #expect(gate.pending == nil)
        }
    }

    @Test func staleRefusalWithANewIdentityDoesNotRearmConfirmation() throws {
        var gate = RebuildGate()
        let check9 = gate.offer(try refusal(identity: identity), session: session, applyingProvider: false)
        #expect(check9)
        _ = gate.confirm(session)
        var changed = identity
        changed["claude_start"] = "new-start"
        let check10 = !gate.offer(try refusal(identity: changed, reason: "confirmation-stale"), session: session, applyingProvider: false)
        #expect(check10)
        #expect(gate.pending == nil)
        #expect(!Restart.arguments(targets: [session.target]).contains("--rebuild-pane"))
    }

}
