import Foundation
import Testing
@testable import MossKit

@MainActor @Suite struct AskTakerTests {
    /// While the app runs it says "here" at once, then every 60 seconds.
    @Test func takerHeartbeatWhileRunning() async {
        var beats = 0
        var waits: [TimeInterval] = []
        let taker = AskTaker(beat: { beats += 1 }, sleep: { seconds in
            waits.append(seconds)
            return waits.count < 3 // the app quits after the third wait
        })
        taker.start()
        await taker.finished()
        #expect(beats == 3)
        #expect(waits == [60, 60, 60])
    }

    @Test func switchedOffNoHeartbeat() async {
        var beats = 0
        let taker = AskTaker(enabled: false, beat: { beats += 1 }, sleep: { _ in true })
        taker.start()
        await taker.finished()
        #expect(beats == 0)
        #expect(!taker.running)
    }

    @Test func switchingOffStopsIt() async {
        var beats = 0
        var taker: AskTaker!
        taker = AskTaker(beat: { beats += 1 }, sleep: { _ in
            if beats == 2 { taker.enabled = false }
            return true
        })
        taker.start()
        await taker.finished()
        #expect(beats == 2)
        #expect(!taker.running)
        // On again: it starts by itself.
        var more = 0
        let again = AskTaker(enabled: false, beat: { more += 1 }, sleep: { _ in false })
        again.enabled = true
        await again.finished()
        #expect(more == 1)
    }

    @Test func theBeatIsAskTake() async {
        let runner = AskRunner()
        let taker = AskTaker(client: MossClient(executable: "/x/cc-moss", runner: runner))
        taker.stop()
        // One beat by hand, the same closure the loop runs.
        try? await MossClient(executable: "/x/cc-moss", runner: runner).take(name: AskTaker.name)
        #expect(runner.calls == [["ask", "take", "--name", "desktop"]])
    }
}
