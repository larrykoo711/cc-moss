import Testing
@testable import MossKit

@Suite struct WorkingSpinTests {
    @Test func ringTurnsWhileThePanelIsShown() {
        #expect(WorkingSpin.runs(panelShown: true, reduceMotion: false))
    }

    @Test func ringStopsWhenPanelHidden() {
        #expect(!WorkingSpin.runs(panelShown: false, reduceMotion: false),
                "an ordered-out panel is still alive; nobody sees the ring turn")
    }

    @Test func ringStopsWithReduceMotion() {
        #expect(!WorkingSpin.runs(panelShown: true, reduceMotion: true))
        #expect(!WorkingSpin.runs(panelShown: false, reduceMotion: true))
    }
}
