import Foundation
import Testing
@testable import MossKit

@Suite struct AskFeedTests {
    private func deskWithDraft() -> AskDesk {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1")]))
        _ = desk.edit("q1") { $0.startWriting(); $0.draft = "use Postgres"; return nil }
        return desk
    }

    @Test func oneFailedListKeepsEverything() {
        var feed = AskFeed()
        var desk = deskWithDraft()
        let change = feed.apply(nil, to: &desk)
        #expect(change == nil, "nothing arrived, nothing went")
        #expect(desk.card("q1")?.draft == "use Postgres", "your words are still there")
        #expect(desk.card("q1")?.writing == true)
    }

    @Test func aFailureDoesNotUntellTheQuestion() {
        // Nothing went, so nothing is withdrawn: the alert's record stays.
        var alerts = AskAlerts()
        let q = asks([askJSON(id: "q1")])
        let first = alerts.arrived(q, slot: { _ in "01A" }, title: { _ in "job-a" }, in: AlertContext(), now: Date())
        var feed = AskFeed()
        var desk = AskDesk()
        _ = feed.apply(q, to: &desk)
        let change = feed.apply(nil, to: &desk)
        #expect(change == nil)
        // Seen again after the hiccup: not a new question, no second alert.
        #expect(alerts.arrived(q, slot: { _ in "01A" }, title: { _ in "job-a" }, in: AlertContext(), now: Date()).isEmpty)
        #expect(first.count == 1)
    }

    @Test func manyFailuresInARowLetTheQuestionsGo() {
        var feed = AskFeed()
        var desk = deskWithDraft()
        for _ in 1..<AskFeed.failuresBeforeReset { #expect(feed.apply(nil, to: &desk) == nil) }
        let change = feed.apply(nil, to: &desk)
        #expect(change?.gone == ["q1"])
        #expect(desk.cards.isEmpty)
    }

    @Test func aGoodListInBetweenStartsTheCountAgain() {
        var feed = AskFeed()
        var desk = deskWithDraft()
        for _ in 1..<AskFeed.failuresBeforeReset { _ = feed.apply(nil, to: &desk) }
        _ = feed.apply(asks([askJSON(id: "q1")]), to: &desk)
        #expect(feed.apply(nil, to: &desk) == nil)
        #expect(desk.card("q1") != nil)
    }

    @Test func aListUpdatesTheDesk() {
        var feed = AskFeed()
        var desk = AskDesk()
        let change = feed.apply(asks([askJSON(id: "q1")]), to: &desk)
        #expect(change?.arrived.map(\.id) == ["q1"])
    }
}
