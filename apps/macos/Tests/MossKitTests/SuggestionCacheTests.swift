import Foundation
import Testing
@testable import MossKit

@Suite struct SuggestionCacheTests {
    let t0 = Date(timeIntervalSince1970: 1_760_600_000)
    private func at(_ s: TimeInterval) -> Date { t0.addingTimeInterval(s) }

    @Test func aSuggestionIsReusedForTheSameTurn() {
        var cache = SuggestionCache()
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: t0) == .ask)
        cache.store("A", marker: "turn-1", value: "run the tests", now: t0)
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(30)) == .known("run the tests"))
    }

    @Test func aNewTurnAsksAgain() {
        var cache = SuggestionCache()
        cache.store("A", marker: "turn-1", value: "run the tests", now: t0)
        #expect(cache.lookup("A", marker: "turn-2", busy: false, now: at(5)) == .ask)
    }

    @Test func busyMeansNothingToAsk() {
        var cache = SuggestionCache()
        cache.store("A", marker: "turn-1", value: "run the tests", now: t0)
        #expect(cache.lookup("A", marker: "turn-1", busy: true, now: at(1)) == .skip)
    }

    @Test func noneIsAskedAgainAtTwoAndSixSeconds() {
        var cache = SuggestionCache()
        cache.store("A", marker: "turn-1", value: nil, now: t0)
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(1)) == .askAt(at(2)))
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(2)) == .ask)
        cache.store("A", marker: "turn-1", value: nil, now: at(2))
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(3)) == .askAt(at(6)))
        cache.store("A", marker: "turn-1", value: nil, now: at(6))
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(7)) == .known(nil), "believed after the third try")
    }

    @Test func aLateSuggestionIsKept() {
        var cache = SuggestionCache()
        cache.store("A", marker: "turn-1", value: nil, now: t0)
        cache.store("A", marker: "turn-1", value: "keep going", now: at(2))
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(3)) == .known("keep going"))
    }

    @Test func entriesLapseAfterTenMinutes() {
        var cache = SuggestionCache()
        cache.store("A", marker: "turn-1", value: "run the tests", now: t0)
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(599)) == .known("run the tests"))
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(600)) == .ask, "same turn, read again")
        cache.store("B", marker: "turn-1", value: nil, now: t0)
        cache.store("B", marker: "turn-1", value: nil, now: at(2))
        cache.store("B", marker: "turn-1", value: nil, now: at(6))
        #expect(cache.lookup("B", marker: "turn-1", busy: false, now: at(300)) == .known(nil))
        #expect(cache.lookup("B", marker: "turn-1", busy: false, now: at(606)) == .ask, "a remembered none lapses too")
    }

    @Test func aReadAfterLapsingStartsFresh() {
        var cache = SuggestionCache()
        cache.store("A", marker: "turn-1", value: "old", now: t0)
        cache.store("A", marker: "turn-1", value: nil, now: at(700))
        #expect(cache.lookup("A", marker: "turn-1", busy: false, now: at(701)) == .askAt(at(702)), "the retries start over")
    }

    @Test func keysAreSeparate() {
        var cache = SuggestionCache()
        cache.store("A", marker: "m", value: "a", now: t0)
        #expect(cache.lookup("B", marker: "m", busy: false, now: t0) == .ask)
        cache.forget("A")
        #expect(cache.lookup("A", marker: "m", busy: false, now: t0) == .ask)
    }
}
