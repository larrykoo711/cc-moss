import Foundation
import Testing
@testable import MossKit

/// The retry loop around the cache, with a fake clock and a fake
/// `cc-moss suggest`.
@Suite struct SuggestionAskTests {
    final class World: @unchecked Sendable {
        var cache = SuggestionCache()
        var now = Date(timeIntervalSince1970: 1_760_600_000)
        var answers: [String?]
        var asked: [Date] = []

        init(_ answers: [String?]) { self.answers = answers }

        func run(busy: Bool = false) async -> String? {
            await SuggestionAsk.run(
                lookup: { self.cache.lookup("A", marker: "m1", busy: busy, now: self.now) },
                ask: {
                    self.asked.append(self.now)
                    return self.answers.isEmpty ? nil : self.answers.removeFirst()
                },
                store: { self.cache.store("A", marker: "m1", value: $0, now: self.now) },
                forget: { self.cache.forget("A") },
                waitUntil: { self.now = max(self.now, $0) })
        }
    }

    @Test func aGuessOnTheFirstTryIsReturned() async {
        let w = World(["run the tests"])
        #expect(await w.run() == "run the tests")
        #expect(w.asked.count == 1)
    }

    @Test func noneIsAskedAgainAtTwoAndSixSeconds() async {
        let w = World([nil, nil, "ok"])
        let start = w.now
        #expect(await w.run() == "ok")
        #expect(w.asked.map { $0.timeIntervalSince(start) } == [0, 2, 6])
    }

    @Test func noneThreeTimesIsBelieved() async {
        let w = World([nil, nil, nil, "too late"])
        #expect(await w.run() == nil)
        #expect(w.asked.count == 3)
    }

    @Test func busyAsksNothing() async {
        let w = World(["x"])
        #expect(await w.run(busy: true) == nil)
        #expect(w.asked.isEmpty)
    }

    @Test func aKnownAnswerIsNotAskedAgain() async {
        let w = World(["x"])
        _ = await w.run()
        #expect(await w.run() == "x")
        #expect(w.asked.count == 1)
    }

    @Test func goneSessionsAreForgotten() {
        var cache = SuggestionCache()
        let t = Date()
        cache.store("A", marker: "m", value: "x", now: t)
        cache.store("B", marker: "m", value: "y", now: t)
        cache.keep(only: ["B"])
        #expect(cache.lookup("A", marker: "m", busy: false, now: t) == .ask)
        #expect(cache.lookup("B", marker: "m", busy: false, now: t) == .known("y"))
    }
}
