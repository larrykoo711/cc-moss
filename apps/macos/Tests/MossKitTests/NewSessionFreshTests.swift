import Foundation
import Testing
@testable import MossKit

@Suite struct NewSessionFreshTests {
    let identity = "00000000-0000-4000-8000-000000000123"

    func receipt(_ result: String = "starting", slot: String? = nil) throws -> NewSessionReceipt {
        let data = try JSONSerialization.data(withJSONObject: ["schema": 1, "result": result,
            "session_id": identity, "name": "project-2", "cwd": "/demo/project",
            "slot_id": slot as Any? ?? NSNull(), "tmux": NSNull()])
        return try NewSessionReceipt.decode(data)
    }

    @Test func unconfirmedWithoutTmuxDoesNotInventARegisteredSession() throws {
        let pending = try receipt("unconfirmed")
        #expect(pending.unconfirmed && pending.tmux == nil)
        #expect(pending.registeredSession(in: sessions([sessionJSON(name: "project-2")])) == nil)
    }

    @Test func receiptMatchesFullIdentityAndOptionalSlotNotName() throws {
        var row = sessions([sessionJSON(id: "A", name: "project-2", status: "running")])[0]
        let pending = try receipt(slot: "A")
        #expect(pending.registeredSession(in: [row]) == nil)
        row.sessionId = identity
        #expect(pending.registeredSession(in: [row])?.id == "A")
        #expect(pending.registeredSession(in: [row, row]) == nil)
        let differentSlot = try receipt(slot: "B")
        #expect(differentSlot.registeredSession(in: [row]) == nil)
        row.tmux = nil
        #expect(pending.registeredSession(in: [row]) == nil)
    }

    @Test func registeredCopyOnlyChangesAfterFullIdentityMatches() throws {
        let pending = try receipt()
        var row = sessions([sessionJSON(name: "project-2", status: "running")])[0]
        L.$override.withValue(.zh) { #expect(pending.statusMessage(in: [row]).contains("等待")) }
        row.sessionId = identity
        L.$override.withValue(.zh) {
            #expect(pending.statusMessage(in: [row]).contains("可打开"))
            #expect(!pending.statusMessage(in: [row]).contains("等待"))
        }
    }

    @Test func directoryFailureCopyIsLocalizedWhileTechnicalDetailsRemain() {
        let raw = "cwd_changed: working directory changed"
        let notice = FailureNotice(action: .newSession, subject: nil, error: MossClient.Failure.command(raw))
        L.$override.withValue(.zh) { #expect(notice.why == WorkingDirectoryReason.changed.explanation) }
        #expect(notice.details.contains(raw))
    }

    @Test func oldCLICannotReceiveJsonNewOrFallback() async {
        let runner = Recorder(stdout: "{}")
        let client = MossClient(executable: "/demo/cc-moss", runner: runner)
        await #expect(throws: MossClient.Failure.self) {
            _ = try await client.createNewSession(name: "project", directory: "/demo/project", capabilities: .init(workingDirectory: 1))
        }
        #expect(await runner.calls.isEmpty)
    }

    @Test func failedReceiptDoesNotSendAnotherNew() async {
        let runner = Recorder(stdout: "{}")
        let client = MossClient(executable: "/demo/cc-moss", runner: runner)
        await #expect(throws: MossClient.Failure.self) {
            _ = try await client.createNewSession(name: "project", directory: "/demo/project", capabilities: .init(workingDirectory: 1, newReceipt: 1))
        }
        #expect(await runner.calls == [["new", "project", "-C", "/demo/project", "--detach", "--json"]])
    }

    @Test(arguments: ["running", "interrupted"])
    func sameNameNeverOpensOrRestoresAnExistingSession(_ status: String) {
        let old = sessions([sessionJSON(name: "project", status: status)])
        let plan = NewSession.plan(name: "project", directory: "/demo/project", sessions: old)
        guard case let .create(arguments) = plan else {
            Issue.record("New session reused an existing identity instead of creating")
            return
        }
        #expect(arguments.contains("--json"))
        #expect(!arguments.contains("--yes"))
    }
}
