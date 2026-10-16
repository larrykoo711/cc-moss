import Foundation
import Testing
@testable import MossKit

@Suite struct IconStateTests {
    @Test func needsYouWinsOverEverything() {
        let list = sessions([
            sessionJSON(id: "A", status: "interrupted"),
            sessionJSON(id: "B", busy: true),
            sessionJSON(id: "C"),
            sessionJSON(id: "D", busy: true, attention: "permission"),
        ])
        #expect(IconState(list) == .needsYou)
    }

    @Test func attentionOnlyCountsWhileRunning() {
        // `ls --json` already drops it for ended sessions; do not lean on that alone.
        let list = sessions([sessionJSON(id: "A", status: "closed", attention: "question")])
        #expect(IconState(list) == .idle)
    }

    @Test func interruptedBeatsWorking() {
        let list = sessions([
            sessionJSON(id: "A", busy: true),
            sessionJSON(id: "B", status: "interrupted"),
        ])
        #expect(IconState(list) == .interrupted)
    }

    @Test func workingBeatsWaiting() {
        let list = sessions([
            sessionJSON(id: "A"),
            sessionJSON(id: "B", busy: true),
        ])
        #expect(IconState(list) == .working)
    }

    @Test func waitingWhenOpenButQuiet() {
        #expect(IconState(sessions([sessionJSON(id: "A")])) == .waiting)
    }

    @Test func idleWhenNothingOpen() {
        #expect(IconState([]) == .idle)
        #expect(IconState(sessions([sessionJSON(id: "A", status: "closed")])) == .idle)
    }

    @Test func badgeCountsTheTopState() {
        let list = sessions([
            sessionJSON(id: "A", status: "interrupted"),
            sessionJSON(id: "B", status: "interrupted"),
            sessionJSON(id: "C", busy: true),
            sessionJSON(id: "D", busy: true, attention: "input"),
        ])
        #expect(IconBadge.count(list) == 1)
        #expect(IconBadge.count(Array(list.prefix(3))) == 2)
        #expect(IconBadge.count([list[2]]) == 1)
    }

    @Test func noBadgeWhenZero() {
        #expect(IconBadge.count([]) == nil)
        // Sessions waiting for you are the normal state of things: no number.
        #expect(IconBadge.count(sessions([sessionJSON(id: "A"), sessionJSON(id: "B")])) == nil)
    }
}
