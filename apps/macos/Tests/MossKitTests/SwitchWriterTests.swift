import Foundation
import Testing
@testable import MossKit

/// A config that takes a moment to write, like `cc-moss config set`.
@MainActor private final class SlowConfig {
    var value = false
    var writes: [Bool] = []
    var fail = false
    func write(_ v: Bool) async throws {
        writes.append(v)
        try await Task.sleep(for: .milliseconds(50))
        if fail { throw CocoaError(.fileWriteUnknown) }
        value = v
    }
    func read() async throws -> Bool { value }
}

@MainActor @Suite struct SwitchWriterTests {
    private func writer(_ c: SlowConfig) -> SwitchWriter {
        SwitchWriter(holds: c.value, write: { try await c.write($0) }, read: { try await c.read() })
    }

    @Test func onThenOffQuicklyEndsOff() async {
        let c = SlowConfig()
        let w = writer(c)
        let first = Task { await w.set(true) }
        // Off while "on" is still being written.
        while c.writes.isEmpty { await Task.yield() }
        let second = await w.set(false)
        let a = await first.value
        #expect(a == .superseded)
        #expect(second == .done(holds: false, failed: nil))
        #expect(c.writes == [true, false])
        #expect(c.value == false)
        #expect(w.requested == false)
    }

    @Test func itSaysWhileAWriteIsUnderWay() async {
        let c = SlowConfig()
        let w = writer(c)
        #expect(!w.isWriting)
        let t = Task { await w.set(true) }
        while c.writes.isEmpty { await Task.yield() }
        #expect(w.isWriting)
        _ = await t.value
        #expect(!w.isWriting)
    }

    @Test func loadingTheSwitchWritesNothing() async {
        let c = SlowConfig()
        c.value = true
        let w = writer(c)
        #expect(await w.set(true) == .unchanged)
        #expect(c.writes.isEmpty)
    }

    @Test func aFailedWriteReportsWhatTheConfigHolds() async {
        let c = SlowConfig()
        c.fail = true
        let w = writer(c)
        guard case .done(let holds, let failed) = await w.set(true) else {
            Issue.record("not done"); return
        }
        #expect(holds == false)
        #expect(failed != nil)
        #expect(w.requested == false)
    }
}
