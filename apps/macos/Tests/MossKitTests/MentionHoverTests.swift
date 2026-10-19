import Foundation
import Testing
@testable import MossKit

@Suite struct MentionHoverTests {
    typealias Machine = MentionHoverMachine

    @Test func aCardShowsAfterAMoment() {
        var m = Machine()
        let effect = m.entered("A")
        #expect(effect == .showLater("A"))
        #expect(m.lit == "A")
        #expect(m.card == nil)
        let shown = m.showFired("A")
        #expect(shown)
        #expect(m.card == "A")
    }

    @Test func leavingBeforeTheMomentShowsNothing() {
        var m = Machine()
        _ = m.entered("A")
        let effect = m.left("A")
        #expect(effect == .hideLater)
        let shown = m.showFired("A")
        #expect(!shown, "the pointer went")
        #expect(m.card == nil)
    }

    @Test func anotherBadgeSwitchesTheCardAtOnce() {
        var m = Machine()
        _ = m.entered("A")
        _ = m.showFired("A")
        _ = m.left("A")
        let effect = m.entered("B")
        #expect(effect == .showNow("B"))
        #expect(m.card == "B")
    }

    @Test func aStaleLeaveIsIgnored() {
        var m = Machine()
        _ = m.entered("A")
        _ = m.entered("B")
        let effect = m.left("A")
        #expect(effect == .none)
        #expect(m.lit == "B")
    }

    @Test func theCardStaysWhileThePointerIsOnIt() {
        var m = Machine()
        _ = m.entered("A")
        _ = m.showFired("A")
        _ = m.left("A")
        let onCard = m.cardHovered(true)
        #expect(onCard == .keep)
        m.hideFired()
        #expect(m.card == "A", "on the card: it stays")
        let offCard = m.cardHovered(false)
        #expect(offCard == .hideLater)
        m.hideFired()
        #expect(m.card == nil)
    }

    @Test func resetClearsEverything() {
        var m = Machine()
        _ = m.entered("A")
        _ = m.showFired("A")
        _ = m.cardHovered(true)
        m.reset()
        #expect(m.lit == nil && m.card == nil)
        _ = m.entered("B")
        let again = m.entered("B")
        #expect(again == .keep, "the same badge again changes nothing")
    }
}
