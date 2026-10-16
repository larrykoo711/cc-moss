import Foundation
import Testing
@testable import MossKit

@Suite struct StartupCheckTests {
    private let list = sessions([
        sessionJSON(id: "A", name: "job-a", status: "interrupted"),
        sessionJSON(id: "B", name: "job-b", status: "interrupted"),
        sessionJSON(id: "C", name: "job-c", status: "running"),
        sessionJSON(id: "D", name: "job-d", status: "closed"),
    ])

    @Test func firstLaunchSaysNothing() {
        #expect(StartupCheck.missed(list, lastRunning: nil).isEmpty)
        #expect(StartupCheck.reminder(list, lastRunning: nil) == .none)
    }

    @Test func runningThenInterruptedIsMissed() {
        #expect(StartupCheck.missed(list, lastRunning: ["A", "C"]).map(\.id) == ["A"])
    }

    @Test func alreadyInterruptedLastTimeIsNot() {
        // B was already interrupted when the app last looked: it was told then.
        #expect(StartupCheck.missed(list, lastRunning: ["C"]).isEmpty)
    }

    @Test func closedIsNot() {
        #expect(StartupCheck.missed(list, lastRunning: ["D"]).isEmpty)
    }

    @Test func oneStaysSingle() {
        #expect(StartupCheck.reminder(list, lastRunning: ["A"]) == .one("A"))
    }

    @Test func severalBecomeOneSummary() {
        #expect(StartupCheck.reminder(list, lastRunning: ["A", "B", "C"]) == .several(["A", "B"]))
    }

    @Test func runningIsWhatStillRuns() {
        #expect(StartupCheck.running(list) == ["C"])
    }
}
