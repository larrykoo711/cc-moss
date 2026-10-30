import Foundation
import Testing
@testable import MossKit

@Suite struct AdoptTests {
    private func one(_ json: [String: Any], role: String? = nil) -> Session {
        var j = json
        if let role { j["role"] = role }
        return sessions([j])[0]
    }

    @Test func anIdleSessionOutsideTmuxCanMoveIn() {
        #expect(Adopt.availability(one(sessionJSON(tmux: false))) == .ready)
    }

    @Test func aWorkingOneWaitsForItsTurnToEnd() {
        #expect(Adopt.availability(one(sessionJSON(busy: true, tmux: false))) == .working)
    }

    @Test func oneAskingYouSomethingWaitsForTheAnswer() {
        #expect(Adopt.availability(one(sessionJSON(tmux: false, attention: "permission"))) == .waitingOnYou)
    }

    @Test func nothingToMoveForTmuxStoppedOrMoss() {
        #expect(Adopt.availability(one(sessionJSON())) == .none, "in tmux already")
        #expect(Adopt.availability(one(sessionJSON(status: "interrupted", tmux: false))) == .none, "bring it back instead")
        #expect(Adopt.availability(one(sessionJSON(status: "closed", tmux: false))) == .none)
        #expect(Adopt.availability(one(sessionJSON(tmux: false), role: "moss")) == .none)
    }

    @Test func aBackgroundSessionOpensInsteadOfMovingIn() {
        let s = one(with(sessionJSON(tmux: false), ["background": true]))
        #expect(s.canOpen)
        #expect(Adopt.availability(s) == .none, "it runs without a terminal")
        let actions = RowActions(s, hopeless: false)
        #expect(actions.primary == .open)
        #expect(!actions.all.joined().contains(.adopt))
        #expect(actions.all.joined().contains(.copyCommand))
        #expect(Message.allowed(s) == false, "nothing types into it")
    }

    @Test func anOlderCliSaysNothingAboutTheBackground() {
        let s = one(sessionJSON(tmux: false))
        #expect(!s.background)
        #expect(!s.canOpen)
    }

    @Test func theRowOffersItFirstAndInItsMenu() {
        let actions = RowActions(one(sessionJSON(tmux: false)), hopeless: false)
        #expect(actions.inline.first == .adopt)
        #expect(actions.more.joined().contains(.adopt))
        #expect(actions.all.joined().contains(.adopt))
        #expect(!RowActions(one(sessionJSON()), hopeless: false).all.joined().contains(.adopt))
    }

    @Test func aBusyRowStillShowsTheButton() {
        #expect(RowActions(one(sessionJSON(busy: true, tmux: false)), hopeless: false).inline.contains(.adopt))
    }

    @Test func noNameOrNoSummaryMeansTidyingUp() {
        #expect(Adopt.tidies(one(sessionJSON(name: nil, tmux: false, lastPrompt: "hi"))))
        #expect(Adopt.tidies(one(sessionJSON(name: "0123abcd", tmux: false, lastPrompt: "hi"))), "looks like an id")
        #expect(!Adopt.tidies(one(sessionJSON(name: "deadbeef-notes", tmux: false, lastPrompt: "hi"))), "a word, not an id")
        #expect(Adopt.tidies(one(sessionJSON(name: "job-a", tmux: false))), "no summary")
        #expect(!Adopt.tidies(one(sessionJSON(name: "job-a", tmux: false, lastPrompt: "hi"))))
        #expect(Adopt.tidies(one(sessionJSON(name: "job-a", tmux: false, lastPrompt: "hi")), named: "acme"))
    }

    @Test func outcomesAreDecoded() throws {
        func decode(_ s: String) throws -> Adopt.Outcome { try Adopt.outcome(Data(s.utf8)) }
        #expect(try decode(#"{"schema":1,"result":"would-move","slot_id":"01A","named":"acme-2","refreshed":true}"#)
            == .wouldMove(named: "acme-2"))
        #expect(try decode(#"{"schema":1,"result":"moved","slot_id":"01A","named":null,"tmux_session":"h_job-a"}"#)
            == .moved(named: nil, tmuxSession: "h_job-a"))
        #expect(try decode(#"{"schema":1,"result":"refused","reason":"busy","message":"It's working"}"#)
            == .refused(reason: "busy", message: "It's working"))
        #expect(try decode(#"{"schema":1,"result":"failed","message":"didn't quit"}"#) == .failed(message: "didn't quit"))
        #expect(throws: MossClient.Failure.self) { try decode(#"{"schema":2,"result":"moved"}"#) }
    }

    @Test func aDryRunAddsItsFlag() {
        #expect(Adopt.arguments(target: "job-a", dryRun: true) == ["adopt", "job-a", "--json", "--dry-run"])
        #expect(Adopt.arguments(target: "job-a", dryRun: false) == ["adopt", "job-a", "--json"])
    }
}
