import Foundation
import Testing
@testable import MossKit

@Suite struct SendFlowTests {
    @Test func oneSendAtATime() {
        var flow = SendFlow()
        let first = flow.begin()
        #expect(first)
        #expect(flow.isSending)
        let second = flow.begin()
        #expect(!second, "no second send while one is on its way")
        #expect(flow == .sending)
    }

    @Test func itEndsIdleOrFailed() {
        var flow = SendFlow()
        _ = flow.begin()
        flow.finish(error: nil)
        #expect(flow == .idle)
        _ = flow.begin()
        flow.finish(error: "the session isn't there any more")
        #expect(flow.failure == "the session isn't there any more")
        let again = flow.begin()
        #expect(again, "after a failure you can try again")
    }

    @Test func theMossRowShowsItsNewestReply() {
        let messages = [
            MossMessage(id: "1", from: .moss, text: "older reply"),
            MossMessage(id: "2", from: .you, text: "hi"),
            MossMessage(id: "3", from: .moss, text: "**api-review** is waiting for you.\nMore below."),
        ]
        #expect(MossRowLine.of(messages) == "api-review is waiting for you.")
    }

    @Test func neverADigestOrACommand() {
        #expect(MossRowLine.of([MossMessage(id: "1", from: .events, text: "[动态] 97-stage5-app 做完了")]) == nil)
        #expect(MossRowLine.of([MossMessage(id: "1", from: .moss, text: "/usage", command: "/usage")]) == nil)
        #expect(MossRowLine.of([MossMessage(id: "1", from: .moss, text: "[Update] x\n- Two things left")]) == "Two things left")
        #expect(MossRowLine.of([MossMessage(id: "1", from: .moss, text: "", fallback: true)]) == nil)
        #expect(MossRowLine.of([]) == nil)
    }
}
