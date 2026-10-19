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
}
