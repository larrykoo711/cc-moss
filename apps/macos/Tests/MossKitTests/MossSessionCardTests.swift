import Foundation
import Testing
@testable import MossKit

@Suite struct MossSessionCardTests {
    @Test func sessionsAreDecodedInOrderWithoutBlanksOrRepeats() throws {
        let json = #"{"schema":1,"messages":[{"id":"1","from":"moss","text":"These two","sessions":["01B"," ","01A","01B"]}]}"#
        let m = try MossChat.decode(Data(json.utf8)).messages[0]
        #expect(m.sessions == ["01B", "01A"])
    }

    @Test func anOlderReplyHasNoSessions() throws {
        let json = #"{"schema":1,"messages":[{"id":"1","from":"moss","text":"hi"}]}"#
        #expect(try MossChat.decode(Data(json.utf8)).messages[0].sessions.isEmpty)
    }

    @Test func anIdNoSessionHasIsAGoneCard() {
        let list = sessions([sessionJSON(id: "01A")])
        let cards = MossSessionCard.cards(["01A", "0ZZ"], among: list)
        #expect(cards.map(\.id) == ["01A", "0ZZ"])
        #expect(!cards[0].isGone)
        #expect(cards[1].isGone)
        #expect(cards[1].main == nil && !cards[1].canRename && !cards[1].canPin)
    }

    @Test func aRunningSessionInTmuxOpensRenamesAndPins() {
        let card = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", busy: true)]))[0]
        #expect(card.main == .open)
        #expect(card.canRename)
        #expect(card.canPin)
    }

    @Test func anInterruptedSessionIsBroughtBackNotRenamed() {
        let card = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", status: "interrupted")]))[0]
        #expect(card.main == .bringBack)
        #expect(!card.canRename)
        #expect(card.canPin)
    }

    @Test func aSessionOutsideTmuxMovesInInsteadOfOpening() {
        let card = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", tmux: false)]))[0]
        #expect(card.main == .moveIntoTmux)
        #expect(card.adopt == .ready)
        #expect(!card.canRename)
        #expect(card.canPin)
    }

    @Test func aBusySessionOutsideTmuxShowsTheMoveWaiting() {
        let busy = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", busy: true, tmux: false)]))[0]
        #expect(busy.main == .moveIntoTmux)
        #expect(busy.adopt == .working)
        let asking = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", tmux: false, attention: "question")]))[0]
        #expect(asking.adopt == .waitingOnYou)
    }

    @Test func onlySessionsOutsideTmuxOfferTheMove() {
        let list = sessions([sessionJSON(id: "01A"), sessionJSON(id: "01B", status: "interrupted", tmux: false)])
        #expect(MossSessionCard.cards(["01A", "01B", "0ZZ"], among: list).allSatisfy { $0.adopt == .none })
    }

    @Test func aClosedSessionOnlyShowsWhatItWas() {
        let card = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", status: "closed")]))[0]
        #expect(card.main == nil && !card.canRename && !card.canPin)
    }

    @Test func aSessionWaitingOnYouIsHandledNotOpened() {
        let list = sessions([sessionJSON(id: "01A", attention: "permission"), sessionJSON(id: "01B", tmux: false)])
        let cards = MossSessionCard.cards(["01A", "01B"], among: list,
                                          waiting: ["01A": .init(reason: .permission, item: .ask("q1")),
                                                    "01B": .init(reason: .input, item: .reply("01B"))])
        #expect(cards[0].main == .handle(.ask("q1")))
        #expect(cards[0].waiting?.reason == .permission)
        #expect(cards[1].main == .handle(.reply("01B")), "handling beats moving into tmux")
    }

    @Test func onceHandledTheCardLooksNormalAgain() {
        let card = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A")]), waiting: [:])[0]
        #expect(card.main == .open)
        #expect(card.waiting == nil)
    }

    @Test func sessionStatesAreDecoded() throws {
        let json = #"{"schema":1,"messages":[{"id":"1","from":"moss","text":"x","sessions":["01A"],"session_states":{"01A":"needs-you-permission"}}]}"#
        #expect(try MossChat.decode(Data(json.utf8)).messages[0].sessionStates == ["01A": "needs-you-permission"])
    }

    @Test func aCardThatNeededYouAndNoLongerDoesIsHandled() {
        let now = sessions([sessionJSON(id: "01A", busy: true)])
        let card = MossSessionCard.cards(["01A"], among: now, then: ["01A": "needs-you-permission"])[0]
        #expect(card.resolved)
        #expect(card.main == .open, "back to Open")
    }

    @Test func stillWaitingIsNotHandled() {
        let list = sessions([sessionJSON(id: "01A", attention: "permission")])
        let inToDo = MossSessionCard.cards(["01A"], among: list, waiting: ["01A": .init(reason: .permission, item: .ask("q"))],
                                           then: ["01A": "needs-you-permission"])[0]
        #expect(!inToDo.resolved)
        let onScreen = MossSessionCard.cards(["01A"], among: list, then: ["01A": "needs-you-permission"])[0]
        #expect(!onScreen.resolved, "attention not cleared yet")
    }

    @Test func withoutAStateFromTheReplyNothingIsHandled() {
        let list = sessions([sessionJSON(id: "01A")])
        #expect(!MossSessionCard.cards(["01A"], among: list)[0].resolved, "older cc-moss")
        #expect(!MossSessionCard.cards(["01A"], among: list, then: ["01A": "working"])[0].resolved, "it never needed you")
    }

    @Test func allHandledOnlyWhenEveryCardIs() {
        let list = sessions([sessionJSON(id: "01A"), sessionJSON(id: "01B", attention: "permission")])
        let then = ["01A": "needs-you-permission", "01B": "needs-you-permission"]
        let some = MossSessionCard.cards(["01A", "01B"], among: list, then: then)
        #expect(some.map(\.resolved) == [true, false])
        #expect(!MossSessionCard.allHandled(some))
        let all = MossSessionCard.cards(["01A", "01B"], among: sessions([sessionJSON(id: "01A"), sessionJSON(id: "01B")]), then: then)
        #expect(MossSessionCard.allHandled(all))
        #expect(!MossSessionCard.allHandled([]))
    }

    @Test func aMergedGroupKeepsEachSessionsState() {
        let a = MossMessage(id: "a", from: .moss, text: "a", sessions: ["01A"], sessionStates: ["01A": "needs-you-permission"])
        let b = MossMessage(id: "b", from: .moss, text: "b", sessions: ["01B"], sessionStates: ["01B": "needs-you-question"])
        #expect(MossGroup(messages: [a, b]).sessionStates == ["01A": "needs-you-permission", "01B": "needs-you-question"])
    }

    @Test func aGoneSessionIsNeverWaiting() {
        let card = MossSessionCard.cards(["0ZZ"], among: [], waiting: ["0ZZ": .init(reason: .question, item: .ask("q"))])[0]
        #expect(card.isGone && card.main == nil && card.waiting == nil)
    }
}
