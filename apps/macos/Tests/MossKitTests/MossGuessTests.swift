import Foundation
import Testing
@testable import MossKit

@Suite struct MossGuessTests {
    @Test func aGuessForTheNewestReplyShows() {
        var guess = MossGuess()
        let ticket = guess.begin(marker: "r1")
        guess.offer("ok, go ahead", for: ticket)
        #expect(guess.shown(newestReply: "r1", waiting: false) == "ok, go ahead")
    }

    @Test func aLateAnswerAfterYouSentSomethingIsDropped() {
        var guess = MossGuess()
        let ticket = guess.begin(marker: "r1")
        guess.sent()
        // The retry that slept through your message comes back now.
        guess.offer("ok, go ahead", for: ticket)
        #expect(guess.shown(newestReply: "r1", waiting: false) == nil)
    }

    @Test func nothingShowsWhileMossIsThinking() {
        var guess = MossGuess()
        guess.offer("ok", for: guess.begin(marker: "r1"))
        #expect(guess.shown(newestReply: "r1", waiting: true) == nil)
    }

    @Test func aGuessForAnOlderReplyDoesntShow() {
        var guess = MossGuess()
        guess.offer("ok", for: guess.begin(marker: "r1"))
        #expect(guess.shown(newestReply: "r2", waiting: false) == nil)
    }

    @Test func aNewerAskWinsOverAnOlderOne() {
        var guess = MossGuess()
        let old = guess.begin(marker: "r1")
        let new = guess.begin(marker: "r2")
        guess.offer("new", for: new)
        guess.offer("old", for: old)
        #expect(guess.shown(newestReply: "r2", waiting: false) == "new")
    }
}
