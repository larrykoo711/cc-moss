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

    @Test func aSessionOutsideTmuxCantBeOpenedOrRenamed() {
        let card = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", tmux: false)]))[0]
        #expect(card.main == nil)
        #expect(!card.canRename)
        #expect(card.canPin)
    }

    @Test func aClosedSessionOnlyShowsWhatItWas() {
        let card = MossSessionCard.cards(["01A"], among: sessions([sessionJSON(id: "01A", status: "closed")]))[0]
        #expect(card.main == nil && !card.canRename && !card.canPin)
    }

    @Test func aMessageWithCardsClosesItsBubble() {
        let at = Date(timeIntervalSince1970: 1_000)
        let a = MossMessage(id: "a", at: at, from: .moss, text: "a", sessions: ["01A"])
        let b = MossMessage(id: "b", at: at.addingTimeInterval(60), from: .moss, text: "b")
        #expect(MossThread.groups([a, b]).count == 2)
    }
}
