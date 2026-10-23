import Foundation
import Testing
@testable import MossKit

@Suite struct RestoreSourcePresentationTests {
    private var session: Session { sessions([sessionJSON(status: "interrupted")])[0] }

    @Test func primaryActionNeedsAnExplicitChoiceAndDisablesAfterConfirmation() throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        #expect(flow[session.id]?.canConfirm == false)
        _ = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        #expect(flow[session.id]?.canConfirm == true)
        _ = flow.confirm(ticket, current: session, reason: .configSourceUnknown)
        #expect(flow[session.id]?.canConfirm == false)
    }

    @Test func changingToDirectoryClearsDefaultUntilPickerReturns() throws {
        var flow = RestoreSourceFlow()
        flow.offer(session, reason: .configSourceUnknown)
        let ticket = try #require(flow[session.id]?.ticket)
        _ = flow.select(.defaultConfig, ticket: ticket, current: session, reason: .configSourceUnknown)
        let pending = flow.beginPicker(current: session, reason: .configSourceUnknown)
        let picker = try #require(pending)
        #expect(flow[session.id]?.canConfirm == false)
        _ = flow.receiveDirectory("/tmp/settings", ticket: picker, current: session, reason: .configSourceUnknown)
        #expect(flow[session.id]?.canConfirm == true)
        #expect(flow[session.id]?.submitting == false)
        _ = flow.select(.defaultConfig, ticket: picker, current: session, reason: .configSourceUnknown)
        #expect(flow[session.id]?.source == .defaultConfig)
        #expect(flow[session.id]?.submitting == false)
    }
}
