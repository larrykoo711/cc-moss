import Foundation
import Testing
@testable import MossKit

/// What the app remembers about alerts stays small however long it runs.
@Suite struct BoundedMemoryTests {
    @Test func aQuestionThatWentIsForgotten() {
        var alerts = AskAlerts()
        let q = asks([askJSON(id: "q1")])
        _ = alerts.arrived(q, slot: { _ in "01A" }, title: { _ in "job-a" }, in: AlertContext(), now: Date())
        #expect(alerts.remembered == 1)
        _ = alerts.gone(["q1"])
        #expect(alerts.remembered == 0)
    }

    @Test func aWithdrawnQuestionIsStillTakenBackOnce() {
        var alerts = AskAlerts()
        let q = asks([askJSON(id: "q1")])
        _ = alerts.arrived(q, slot: { _ in "01A" }, title: { _ in "job-a" }, in: AlertContext(petEnabled: true, petVisible: true), now: Date())
        #expect(alerts.gone(["q1"]).count == 1)
        #expect(alerts.gone(["q1"]).isEmpty)
    }

    @Test func mossRepliesSeenAreCapped() {
        var watch = MossReplyWatch()
        _ = watch.arrived([])
        for i in 0..<(MossReplyWatch.memory + 50) {
            watch.sent("y\(i)")
            _ = watch.arrived([MossMessage(id: "m\(i)", from: .moss, text: "ok", replyTo: "y\(i)")])
        }
        #expect(watch.remembered.seen <= MossReplyWatch.memory)
        #expect(watch.remembered.sent <= MossReplyWatch.memory)
    }

    @Test func aReplyIsStillToldOnlyOnce() {
        var watch = MossReplyWatch()
        _ = watch.arrived([])
        watch.sent("y1")
        let r = MossMessage(id: "m1", from: .moss, text: "ok", replyTo: "y1")
        #expect(watch.arrived([r]).map(\.id) == ["m1"])
        #expect(watch.arrived([r]).isEmpty)
    }
}
