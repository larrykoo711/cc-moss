import Foundation
import Testing
@testable import MossKit

@Suite struct MossThreadTests {
    let start = Date(timeIntervalSince1970: 1_760_600_000)

    private func moss(_ id: String, _ minute: Double, buttons: [String] = [], command: String? = nil,
                      refused: Bool = false, fallback: Bool = false) -> MossMessage {
        MossMessage(id: id, at: start.addingTimeInterval(minute * 60), from: .moss, text: "m\(id)",
                    fallback: fallback, buttons: buttons, command: command, refused: refused)
    }

    private func you(_ id: String, _ minute: Double, _ text: String = "hi", _ delivery: MossMessage.Delivery? = .replied) -> MossMessage {
        MossMessage(id: id, at: start.addingTimeInterval(minute * 60), from: .you, text: text, delivery: delivery)
    }

    private func shape(_ messages: [MossMessage]) -> [[String]] {
        MossThread.groups(messages).map { $0.messages.map(\.id) }
    }

    @Test func aRunOfShortRepliesIsOneBubble() {
        // The screenshot: six lines over half an hour, then one 40 minutes later.
        let lines = [0, 4, 9, 15, 22, 30].enumerated().map { moss("\($0.offset)", Double($0.element)) }
        #expect(shape(lines + [moss("6", 70)]) == [["0", "1", "2", "3", "4", "5"], ["6"]])
    }

    @Test func eachStepCountsNotTheWholeRun() {
        #expect(shape([moss("a", 0), moss("b", 10), moss("c", 20)]) == [["a", "b", "c"]], "10 minutes apart each")
        #expect(shape([moss("a", 0), moss("b", 10.5)]) == [["a"], ["b"]], "just over 10 minutes")
    }

    @Test func theOtherSideSplits() {
        #expect(shape([moss("a", 0), you("b", 1), moss("c", 2)]) == [["a"], ["b"], ["c"]])
    }

    @Test func yourQuickMessagesMergeToo() {
        #expect(shape([you("a", 0), you("b", 0.5), moss("c", 1)]) == [["a", "b"], ["c"]])
    }

    @Test func commandsAndReceiptsStandAlone() {
        #expect(shape([moss("a", 0), moss("card", 1, command: "/usage"), moss("b", 2)]) == [["a"], ["card"], ["b"]])
        #expect(shape([moss("a", 0), moss("no", 1, command: "/login", refused: true)]) == [["a"], ["no"]])
        #expect(shape([moss("a", 0), moss("got", 1, fallback: true)]) == [["a"], ["got"]])
        #expect(shape([you("a", 0), you("u", 0.5, "/usage")]) == [["a"], ["u"]], "your command goes with its card")
    }

    @Test func buttonsCloseTheGroup() {
        #expect(shape([moss("a", 0), moss("b", 1, buttons: ["yes"]), moss("c", 2)]) == [["a", "b"], ["c"]])
    }

    @Test func aFailedMessageKeepsItsResendBesideIt() {
        #expect(shape([you("a", 0), you("b", 1, "x", .failed), you("c", 2)]) == [["a", "b"], ["c"]])
    }

    @Test func withoutTimesNothingMerges() {
        let a = MossMessage(id: "a", from: .moss, text: "a")
        let b = MossMessage(id: "b", from: .moss, text: "b")
        #expect(shape([a, b]) == [["a"], ["b"]])
    }

    @Test func theGroupIsNamedByItsFirstMessage() {
        let groups = MossThread.groups([moss("a", 0), moss("b", 1)])
        #expect(groups.map(\.id) == ["a"])
        #expect(groups[0].last.id == "b")
    }
}
