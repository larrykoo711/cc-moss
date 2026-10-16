import Foundation
import Testing
@testable import MossKit

@Suite struct WelcomeFlowTests {
    private func defaults() -> UserDefaults { UserDefaults(suiteName: "WelcomeFlowTests-\(UUID().uuidString)")! }

    @Test func startsAtWelcome() {
        let flow = WelcomeFlow()
        #expect(flow.step == .welcome)
        #expect(!flow.canGoBack)
        #expect(flow.progress == (1, WelcomeFlow.Step.allCases.count))
    }

    @Test func nextAndBack() {
        var flow = WelcomeFlow()
        flow.next()
        #expect(flow.step == .tools)
        #expect(flow.canGoBack)
        flow.back()
        #expect(flow.step == .welcome)
        flow.back()
        #expect(flow.step == .welcome, "nothing before the first step")
    }

    @Test func skipStepMarksSkipped() {
        var flow = WelcomeFlow()
        flow.next()
        flow.skipStep()
        #expect(flow.step == .startup)
        #expect(flow.state(of: .tools) == .skipped)
    }

    @Test func satisfiedStepsAreTicked() {
        var flow = WelcomeFlow()
        flow.satisfied([.tools, .startup])
        #expect(flow.state(of: .tools) == .done)
        #expect(flow.state(of: .startup) == .done)
        #expect(flow.state(of: .terminal) == .pending)
        // Something satisfied later ticks too, but never un-ticks.
        flow.satisfied([])
        #expect(flow.state(of: .tools) == .done)
    }

    @Test func lastStepFinishes() {
        var flow = WelcomeFlow()
        for _ in WelcomeFlow.Step.allCases { flow.next() }
        #expect(flow.step == .done)
        #expect(flow.isFinished)
    }

    @Test func finishedOrSkippedStaysDone() {
        let d = defaults()
        #expect(WelcomeFlow.shouldShow(d))
        WelcomeFlow.markSeen(d)
        #expect(!WelcomeFlow.shouldShow(d))
    }

    @Test func reopenFromSettingsStartsOver() {
        var flow = WelcomeFlow()
        flow.next()
        flow.next()
        flow = WelcomeFlow()
        #expect(flow.step == .welcome)
    }
}
