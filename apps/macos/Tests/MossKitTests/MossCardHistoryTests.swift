import Foundation
import Testing
@testable import MossKit

@Suite struct MossCardHistoryTests {
    private let t0 = Date(timeIntervalSince1970: 1_760_600_000)

    private func moss(_ id: String, _ minute: Double, _ sessions: [String], states: [String: String] = [:]) -> MossMessage {
        MossMessage(id: id, at: t0.addingTimeInterval(minute * 60), from: .moss, text: id,
                    sessions: sessions, sessionStates: states)
    }

    private func you(_ id: String, _ minute: Double) -> MossMessage {
        MossMessage(id: id, at: t0.addingTimeInterval(minute * 60), from: .you, text: "?", delivery: .replied)
    }

    private let waiting = ["01A": "needs-you-permission"]

    @Test func onlyTheNewestCardOfASessionIsLive() {
        let groups = MossThread.groups([
            moss("a", 0, ["01A"], states: waiting), you("y1", 20),
            moss("b", 30, ["01A"], states: waiting), you("y2", 50),
            moss("c", 60, ["01A", "01B"], states: ["01A": "needs-you-permission", "01B": "working"]),
        ])
        let roles = MossCardHistory.roles(groups)
        #expect(roles["c"]?["01A"] == .live)
        #expect(roles["c"]?["01B"] == .live)
        // Every later moment still needed you, and now is unknown: nothing
        // says these were handled.
        #expect(roles["a"]?["01A"] == .receipt(.past))
        #expect(roles["b"]?["01A"] == .receipt(.past))
    }

    @Test func aNewerCardThatNoLongerNeedsYouMakesOlderOnesHandled() {
        let groups = MossThread.groups([
            moss("a", 0, ["01A"], states: waiting), you("y1", 20),
            moss("b", 30, ["01A"], states: ["01A": "working"]), you("y2", 50),
            moss("c", 60, ["01A"], states: waiting),
        ])
        let roles = MossCardHistory.roles(groups)
        #expect(roles["a"]?["01A"] == .receipt(.handled), "b said it was working again")
        #expect(roles["b"]?["01A"] == .receipt(.past), "it didn't need you then")
        #expect(roles["c"]?["01A"] == .live)
    }

    @Test func itsStateNowCountsToo() {
        let groups = MossThread.groups([
            moss("a", 0, ["01A"], states: waiting), you("y", 20),
            moss("b", 30, ["01A"], states: waiting),
        ])
        let handled = MossCardHistory.roles(groups, needsYouNow: { _ in false })
        #expect(handled["a"]?["01A"] == .receipt(.handled), "it doesn't need you now")
        let still = MossCardHistory.roles(groups, needsYouNow: { _ in true })
        #expect(still["a"]?["01A"] == .receipt(.past), "it still needs you: maybe for that same thing")
    }

    @Test func anOlderCardThatDidntNeedYouIsJustEarlier() {
        let groups = MossThread.groups([
            moss("a", 0, ["01A"], states: ["01A": "working"]), you("y", 20),
            moss("b", 30, ["01A"]),
        ])
        let roles = MossCardHistory.roles(groups, needsYouNow: { _ in false })
        #expect(roles["a"]?["01A"] == .receipt(.past))
        #expect(roles["b"]?["01A"] == .live)
    }

    @Test func withoutAStateFromTheReplyAnOlderCardIsEarlier() {
        let groups = MossThread.groups([moss("a", 0, ["01A"]), you("y", 20), moss("b", 30, ["01A"])])
        #expect(MossCardHistory.roles(groups, needsYouNow: { _ in false })["a"]?["01A"] == .receipt(.past))
    }

    @Test func aSessionNamedOnceStaysLive() {
        let groups = MossThread.groups([moss("a", 0, ["01A"]), you("y", 20), moss("b", 30, ["01B"])])
        let roles = MossCardHistory.roles(groups)
        #expect(roles["a"]?["01A"] == .live)
        #expect(roles["b"]?["01B"] == .live)
    }
}
