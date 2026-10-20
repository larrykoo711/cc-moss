import Foundation
import Testing
@testable import MossKit

@Suite struct WaitingCardTests {
    let list = sessions([
        sessionJSON(id: "01A", name: "api", attention: "permission"),
        sessionJSON(id: "01B", name: "web", attention: "question"),
        sessionJSON(id: "01C", name: "docs", lastActive: "2026-10-18T09:00:00.000Z", attention: "input"),
        sessionJSON(id: "01D", name: "idle"),
    ])

    private func card(_ id: String, kind: String = "question", session: String, at: String,
                      questions: [[String: Any]]? = nil) -> AskCard {
        AskCard(ask(with(askJSON(id: id, kind: kind, session: session, questions: questions), ["created_at": at])))
    }

    private var cards: [AskCard] {
        [card("q1", session: "01B-sid-0000", at: "2026-10-18T10:02:00Z"),
         card("p1", kind: "permission", session: "01A-sid-0000", at: "2026-10-18T10:01:00Z"),
         card("p2", kind: "permission", session: "01A-sid-0000", at: "2026-10-18T10:03:00Z")]
    }

    private var rows: [WaitingRow] {
        WaitingCard.rows(cards: cards, replies: AskDesk.waitingForInput(list, asks: cards.map(\.ask)), sessions: list)
    }

    @Test func oneRowPerSessionOldestFirst() {
        #expect(rows.map(\.id) == ["01C", "01A", "01B"])
    }

    @Test func aSessionWithTwoAsksShowsTheOldestPlusOne() {
        let api = rows.first { $0.id == "01A" }!
        #expect(api.item == .ask("p1"))
        #expect(api.more == 1)
    }

    @Test func eachRowSaysWhatItWantsInPlainWords() {
        L.$override.withValue(.zh) {
            #expect(rows.map(\.line) == ["等你回话", "Bash · rm -rf build/", "Which database?"])
        }
    }

    @Test func aSimpleQuestionCarriesItsAnswers() {
        #expect(rows.last?.want == .question(text: "Which database?", options: ["Postgres", "SQLite"]))
    }

    @Test func aMultipleChoiceOpensTheFullQuestionInstead() {
        let c = card("m1", session: "01B-sid-0000", at: "2026-10-18T10:00:00Z", questions: [questionJSON(multi: true)])
        let r = WaitingCard.rows(cards: [c], replies: [], sessions: list)
        #expect(r.first?.want == .question(text: "Which database?", options: []))
    }

    @Test func anAskBeingSentIsNoLongerARow() {
        var desk = AskDesk()
        desk.update(cards.map(\.ask))
        _ = desk.edit("q1") { $0.choose("Postgres") }
        let r = WaitingCard.rows(cards: desk.cards, replies: [], sessions: list)
        #expect(!r.contains { $0.id == "01B" })
    }

    @Test func theTitleCountsEveryAsk() {
        L.$override.withValue(.en) { #expect(WaitingCard.title(rows) == "Waiting on you · 4") }
        L.$override.withValue(.zh) { #expect(WaitingCard.title(rows) == "等你处理 · 4") }
    }

    @Test func nothingWaitingNoRows() {
        #expect(WaitingCard.rows(cards: [], replies: [], sessions: list).isEmpty)
    }

    // MARK: Receipt

    let t0 = parseTimestamp("2026-10-18T10:00:00Z")!

    @Test func handlingTheLastOneLeavesAQuietReceipt() {
        var r = WaitingReceipt()
        r.update([.ask("p1"), .reply("01C")], now: t0)
        #expect(r.receipt(now: t0) == nil)
        r.update([.reply("01C")], now: t0)
        #expect(r.receipt(now: t0) == nil, "one left: still the card")
        r.update([], now: t0)
        #expect(r.receipt(now: t0.addingTimeInterval(1)) == 2)
    }

    @Test func theReceiptGoesAfterTenSeconds() {
        var r = WaitingReceipt()
        r.update([.ask("p1")], now: t0)
        r.update([], now: t0)
        #expect(r.receipt(now: t0.addingTimeInterval(9.9)) == 1)
        #expect(r.receipt(now: t0.addingTimeInterval(10)) == nil)
    }

    @Test func theReceiptGoesWithTheNextMessage() {
        var r = WaitingReceipt()
        r.update([.ask("p1")], now: t0)
        r.update([], now: t0)
        r.messageArrived()
        #expect(r.receipt(now: t0) == nil)
    }

    @Test func somethingNewEndsTheReceiptAndCountsAfresh() {
        var r = WaitingReceipt()
        r.update([.ask("p1")], now: t0)
        r.update([], now: t0)
        r.update([.ask("p9")], now: t0)
        #expect(r.receipt(now: t0) == nil)
        r.update([], now: t0)
        #expect(r.receipt(now: t0) == 1)
    }

    @Test func nothingHandledNoReceipt() {
        var r = WaitingReceipt()
        r.update([], now: t0)
        #expect(r.receipt(now: t0) == nil)
    }

    @Test func receiptWords() {
        L.$override.withValue(.zh) { #expect(WaitingReceipt.text(2) == "刚处理了 2 个请求") }
        L.$override.withValue(.en) { #expect(WaitingReceipt.text(2) == "Handled 2 just now") }
    }

    // MARK: Which mentions get a card

    private func mention(_ json: [String: Any], waiting: MossSessionCard.Waiting? = nil) -> MossSessionCard {
        let s = sessions([json])
        return MossSessionCard.cards([s[0].id], among: s, waiting: waiting.map { [s[0].id: $0] } ?? [:])[0]
    }

    @Test func anIdleSessionGetsNoCard() {
        #expect(!mention(sessionJSON(id: "01D")).shown)
    }

    @Test func aWorkingSessionGetsNoCard() {
        #expect(!mention(sessionJSON(id: "01D", busy: true)).shown)
    }

    @Test func anInterruptedSessionGetsItsBringBackCard() {
        #expect(mention(sessionJSON(id: "01D", status: "interrupted")).shown)
    }

    @Test func anEndedOrGoneSessionStillGetsItsCard() {
        #expect(mention(sessionJSON(id: "01D", status: "closed")).shown)
        #expect(MossSessionCard.cards(["0ZZ"], among: []).first?.shown == true)
    }

    @Test func aSessionWaitingOnYouIsInTheWaitingCardNotAMentionCard() {
        #expect(!mention(sessionJSON(id: "01A", attention: "permission")).shown)
        #expect(!mention(sessionJSON(id: "01C", attention: "input"),
                         waiting: .init(reason: .input, item: .reply("01C"))).shown)
    }

    @Test func outsideTmuxItStillOffersTheMove() {
        #expect(mention(sessionJSON(id: "01D", tmux: false)).shown)
    }
}
