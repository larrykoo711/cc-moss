import Foundation
import Testing
@testable import MossKit

@Suite struct AskWithdrawTests {
    @Test func newQuestionsArrive() {
        var desk = AskDesk()
        let change = desk.update(asks([askJSON(id: "q1"), askJSON(id: "q2", kind: "permission")]))
        #expect(change.arrived.map(\.id) == ["q1", "q2"])
        #expect(change.gone.isEmpty)
        #expect(desk.cards.map(\.id) == ["q1", "q2"])
        // The same list again: nothing new.
        #expect(desk.update(asks([askJSON(id: "q1"), askJSON(id: "q2", kind: "permission")])) == AskDesk.Change())
    }

    /// Answered in the terminal, timed out, or the session ended: the
    /// question leaves `ask list`, and its card goes with it.
    @Test func cardWithdrawsWhenClosed() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1"), askJSON(id: "q2")]))
        let change = desk.update(asks([askJSON(id: "q2")]))
        #expect(change.gone == ["q1"])
        #expect(desk.card("q1") == nil)
        #expect(desk.card("q2") != nil)
    }

    @Test func aChoosingCardGoesAtOnce() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1", questions: [questionJSON("Any?", labels: ["x", "y"], multi: true)])]))
        _ = desk.edit("q1") { $0.choose("x") }
        #expect(desk.card("q1")?.phase == .choosing)
        let change = desk.update([])
        #expect(change.gone == ["q1"])
        #expect(desk.cards.isEmpty)
    }

    @Test func aSentCardStaysToSayHowItWent() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1")]))
        let sent = desk.edit("q1") { $0.choose("SQLite") }
        #expect(sent == .choices([["SQLite"]]))
        // Our own answer closes it; the card waits for the outcome.
        #expect(desk.update([]).gone == ["q1"])
        #expect(desk.card("q1")?.phase == .submitting)
        desk.finish("q1", .answered)
        #expect(desk.card("q1")?.phase == .finished(.answered))
        #expect(desk.open.isEmpty)
        desk.clear("q1")
        #expect(desk.cards.isEmpty)
    }

    @Test func goneIsReportedOnce() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1")]))
        _ = desk.edit("q1") { $0.choose("SQLite") }
        #expect(desk.update([]).gone == ["q1"])
        #expect(desk.update([]).gone.isEmpty)
    }

    @Test func aClearedCardDoesNotComeBack() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1")]))
        _ = desk.edit("q1") { $0.choose("SQLite") }
        desk.finish("q1", .notWaiting)
        desk.clear("q1")
        // Still listed for a moment: no second card for it.
        #expect(desk.update(asks([askJSON(id: "q1")])).arrived.isEmpty)
        #expect(desk.cards.isEmpty)
    }

    /// `ask list` failed (or the feature is off): no cards, no error, and
    /// what was said about the questions is taken back.
    @Test func aFailedListShowsNothing() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1")]))
        let change = desk.reset()
        #expect(desk.cards.isEmpty)
        #expect(change.gone == ["q1"])
    }

    @Test func theNameArrivesLater() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1", name: nil)]))
        desk.update(asks([askJSON(id: "q1", name: "job-a")]))
        #expect(desk.card("q1")?.ask.sessionName == "job-a")
    }

    @Test func theFocusedCardComesFirstAndGoesWithIt() {
        var desk = AskDesk()
        desk.update(asks([askJSON(id: "q1"), askJSON(id: "q2")]))
        desk.focused = "q2"
        #expect(desk.ordered.map(\.id) == ["q2", "q1"])
        desk.update(asks([askJSON(id: "q1")]))
        #expect(desk.focused == nil)
    }

    @Test func waitingForInputSkipsQuestionsMossAndOutsideTmux() {
        let list = sessions([
            sessionJSON(id: "A", name: "typing", attention: "input"),
            sessionJSON(id: "B", name: "asking", attention: "input"),
            sessionJSON(id: "C", name: "plain"),
            sessionJSON(id: "D", name: "no-tmux", tmux: false, attention: "input"),
        ])
        let open = asks([askJSON(session: "B-sid-0000")])
        #expect(AskDesk.waitingForInput(list, asks: open).map(\.id) == ["A"])
    }

    @Test func titlesComeFromThePanel() {
        let list = sessions([sessionJSON(id: "01A", name: "renamed")])
        #expect(AskDesk.sessionTitle(ask(askJSON(name: "job-a")), sessions: list) == "renamed")
        #expect(AskDesk.sessionTitle(ask(askJSON(session: "zz-unknown-id", name: "job-a")), sessions: list) == "job-a")
        #expect(AskDesk.sessionTitle(ask(askJSON(session: "zz-unknown-id", name: nil)), sessions: list) == "zz-unkno")
        #expect(AskDesk.slot(ask(), sessions: list) == "01A")
    }
}
