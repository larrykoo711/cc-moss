import Foundation
import Testing
@testable import MossKit

@Suite struct MossWaitingTests {
    private func you(_ id: String, _ text: String = "hi", _ delivery: MossMessage.Delivery? = .sent) -> MossMessage {
        MossMessage(id: id, from: .you, text: text, delivery: delivery)
    }

    private func moss(_ id: String, to: String?) -> MossMessage {
        MossMessage(id: id, from: .moss, text: "ok", replyTo: to)
    }

    @Test func aJustSentMessageWaits() {
        #expect(MossWaiting.of([you("1", "hi", .sending)], activity: .idle) == MossWaiting())
        #expect(MossWaiting.of([you("1", "hi", .sent)], activity: .idle) == MossWaiting())
        #expect(MossWaiting.of([you("1", "hi", .queued)], activity: .idle) == MossWaiting(), "Moss is free: being handed over")
    }

    @Test func aCommandSaysWhatsRunning() {
        #expect(MossWaiting.of([you("1", "/usage", .queued)], activity: .idle) == MossWaiting(command: "/usage"))
        #expect(MossWaiting.of([you("1", "/Context now", .sent)], activity: nil)?.command == "/context")
    }

    @Test func queuedWhileMossIsBusy() {
        #expect(MossWaiting.of([you("1", "hi", .queued)], activity: .thinking) == MossWaiting(queued: true))
        #expect(MossWaiting.of([you("1", "/usage", .queued)], activity: .waiting) == MossWaiting(command: "/usage", queued: true))
    }

    @Test func aReplyEndsIt() {
        #expect(MossWaiting.of([you("1"), moss("2", to: "1")], activity: .idle) == nil)
        #expect(MossWaiting.of([you("1", "hi", .replied)], activity: .idle) == nil)
        #expect(MossWaiting.of([you("1"), moss("2", to: nil)], activity: .idle) == nil, "a reply cc-moss couldn't tie back")
    }

    @Test func aFailureEndsIt() {
        #expect(MossWaiting.of([you("1", "hi", .failed)], activity: .idle) == nil)
    }

    @Test func onlyTheNewestCounts() {
        let messages = [you("1"), moss("2", to: "1"), you("3", "/stats", .sent)]
        #expect(MossWaiting.of(messages, activity: .idle) == MossWaiting(command: "/stats"))
        #expect(MossWaiting.of([you("1"), you("3", "hi", .failed)], activity: .idle) == nil)
    }

    @Test func mossThinkingOnItsOwnStillShows() {
        #expect(MossWaiting.of([], activity: .thinking) == MossWaiting())
        #expect(MossWaiting.of([you("1"), moss("2", to: "1")], activity: .thinking) == MossWaiting())
        #expect(MossWaiting.of([], activity: .idle) == nil)
    }

    @Test func digestsDontCountAsAReply() {
        let digest = MossMessage(id: "2", from: .events, text: "[events] x")
        #expect(MossWaiting.of([you("1"), digest], activity: .idle) == MossWaiting())
    }
}
