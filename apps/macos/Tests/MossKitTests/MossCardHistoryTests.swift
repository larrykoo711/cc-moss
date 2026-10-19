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

    @Test func onlyTheNewestCardOfASessionIsLive() {
        let groups = MossThread.groups([
            moss("a", 0, ["01A"], states: ["01A": "needs-you-permission"]), you("y1", 20),
            moss("b", 30, ["01A"], states: ["01A": "needs-you-permission"]), you("y2", 50),
            moss("c", 60, ["01A", "01B"], states: ["01A": "needs-you-permission", "01B": "working"]),
        ])
        let roles = MossCardHistory.roles(groups)
        #expect(roles["c"]?["01A"] == .live)
        #expect(roles["c"]?["01B"] == .live)
        #expect(roles["a"]?["01A"] == .receipt(.handled(at: nil)), "it needed you then; a newer card took over")
        #expect(roles["b"]?["01A"] == .receipt(.handled(at: nil)))
    }

    @Test func anOlderCardThatDidntNeedYouIsJustPast() {
        let groups = MossThread.groups([
            moss("a", 0, ["01A"], states: ["01A": "working"]), you("y", 20),
            moss("b", 30, ["01A"]),
        ])
        let roles = MossCardHistory.roles(groups)
        #expect(roles["a"]?["01A"] == .receipt(.past))
        #expect(roles["b"]?["01A"] == .live)
    }

    @Test func withoutAStateFromTheReplyAnOlderCardIsPast() {
        let groups = MossThread.groups([moss("a", 0, ["01A"]), you("y", 20), moss("b", 30, ["01A"])])
        #expect(MossCardHistory.roles(groups)["a"]?["01A"] == .receipt(.past))
    }

    @Test func theResolvedTimeIsUsedWhenKnown() {
        let groups = MossThread.groups([
            moss("a", 0, ["01A"], states: ["01A": "needs-you-permission"]), you("y", 20),
            moss("b", 30, ["01A"]),
        ])
        let at = t0.addingTimeInterval(5 * 60)
        // The session left "needs you" five minutes after the first reply.
        let roles = MossCardHistory.roles(groups, resolvedAt: { id, after in id == "01A" && after < at ? at : nil })
        #expect(roles["a"]?["01A"] == .receipt(.handled(at: at)))
    }

    @Test func aSessionNamedOnceStaysLive() {
        let groups = MossThread.groups([moss("a", 0, ["01A"]), you("y", 20), moss("b", 30, ["01B"])])
        let roles = MossCardHistory.roles(groups)
        #expect(roles["a"]?["01A"] == .live)
        #expect(roles["b"]?["01B"] == .live)
    }
}
