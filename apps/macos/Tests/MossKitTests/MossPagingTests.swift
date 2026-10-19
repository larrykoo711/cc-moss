import Foundation
import Testing
@testable import MossKit

@Suite struct MossPagingTests {
    @Test func pagesGrowByThirtyUpToAHundred() {
        #expect(MossPaging.next(after: 30) == 60)
        #expect(MossPaging.next(after: 60) == 90)
        #expect(MossPaging.next(after: 90) == 100)
        #expect(MossPaging.next(after: 100) == nil)
    }

    @Test func theTopOffersMoreWhileThereMayBeMore() {
        #expect(MossPaging.top(limit: 30, loaded: 30) == .loadEarlier)
        #expect(MossPaging.top(limit: 90, loaded: 90) == .loadEarlier)
        #expect(MossPaging.top(limit: 100, loaded: 100) == .terminal)
    }

    @Test func aShortHistoryOffersNothing() {
        #expect(MossPaging.top(limit: 30, loaded: 12) == .nothing)
        #expect(MossPaging.top(limit: 60, loaded: 41) == .nothing)
        #expect(MossPaging.top(limit: 100, loaded: 99) == .nothing)
    }

    @Test func theLimitGoesToTheCLI() async throws {
        let runner = AskRunner([(["moss", "chat"], ok(#"{"schema":1,"messages":[]}"#))])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        _ = try await client.mossChat(limit: 60)
        #expect(runner.calls == [["moss", "chat", "--json", "--limit", "60"]])
    }

    @Test func longRepliesFold() {
        #expect(!MossFold.folds(120))
        #expect(!MossFold.folds(MossFold.height + MossFold.slack), "only a little taller: left open")
        #expect(MossFold.folds(MossFold.height + MossFold.slack + 1))
        #expect(MossFold.folds(900))
    }

    @Test func digestsAreNotShown() {
        let messages = [
            MossMessage(id: "1", from: .events, text: "[events] api-review is waiting"),
            MossMessage(id: "2", from: .moss, text: "Heads up: api-review needs you.", replyTo: "1"),
            MossMessage(id: "3", from: .you, text: "thanks"),
        ]
        #expect(MossDesk.visible(messages).map(\.id) == ["2", "3"], "Moss's reply to a digest still shows")
    }

    @MainActor @Test func theDeskHidesDigests() {
        let desk = MossDesk { _ in "" }
        desk.update(chat: MossChat(messages: [MossMessage(id: "1", from: .events, text: "[events] x"),
                                              MossMessage(id: "2", from: .moss, text: "hi")]))
        #expect(desk.messages.map(\.id) == ["2"])
        #expect(desk.chat.count == 2, "the log itself is untouched")
    }
}
