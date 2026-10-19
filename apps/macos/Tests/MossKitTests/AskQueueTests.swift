import Foundation
import Testing
@testable import MossKit

@Suite struct AskQueueTests {
    private let abc: [AskQueue.Item] = [.ask("a"), .ask("b"), .reply("s1")]

    @Test func startsCollapsedOnTheFirstItem() {
        let q = AskQueue()
        #expect(!q.expanded)
        #expect(q.shown(in: abc) == .ask("a"))
        #expect(q.shown(in: []) == nil)
    }

    @Test func questionsComeFirstThenReplies() {
        let cards = asks([askJSON(id: "q1"), askJSON(id: "q2")]).map(AskCard.init)
        let items = AskQueue.items(cards: cards, replies: [])
        #expect(items == [.ask("q1"), .ask("q2")])
    }

    @Test func stepsStopAtTheEnds() {
        var q = AskQueue()
        q.step(-1, in: abc)
        #expect(q.shown(in: abc) == .ask("a"))
        q.step(1, in: abc)
        q.step(1, in: abc)
        q.step(1, in: abc)
        #expect(q.shown(in: abc) == .reply("s1"))
        #expect(q.index(in: abc) == 2)
    }

    @Test func answeringOneMovesOnToTheNext() {
        var q = AskQueue()
        q.step(1, in: abc) // on "b"
        let after: [AskQueue.Item] = [.ask("a"), .reply("s1")]
        #expect(q.shown(in: after) == .reply("s1"))
    }

    @Test func answeringTheLastShowsTheOneBefore() {
        var q = AskQueue()
        q.step(2, in: abc) // on "s1"
        #expect(q.shown(in: [.ask("a"), .ask("b")]) == .ask("b"))
    }

    @Test func stayOnYourItemWhenOthersArrive() {
        var q = AskQueue()
        q.step(1, in: abc) // on "b"
        #expect(q.shown(in: [.ask("new")] + abc) == .ask("b"))
    }

    @Test func showOpensOnThatItem() {
        var q = AskQueue()
        q.show(.reply("s1"), in: abc)
        #expect(q.expanded)
        #expect(q.shown(in: abc) == .reply("s1"))
    }

    @Test func emptiedFoldsAwayAndStartsOver() {
        var q = AskQueue()
        q.show(.ask("b"), in: abc)
        q.emptied()
        #expect(!q.expanded)
        #expect(q.shown(in: abc) == .ask("a"))
    }
}
