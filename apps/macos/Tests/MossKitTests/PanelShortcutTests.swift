import Foundation
import Testing
@testable import MossKit

@Suite struct PanelShortcutTests {
    private func tap(_ d: inout DoubleTap, at t: TimeInterval, hold: TimeInterval = 0.08) -> Bool {
        _ = d.modifiers(key: true, others: false, at: t)
        return d.modifiers(key: false, others: false, at: t + hold)
    }

    @Test func twoQuickTapsFire() {
        var d = DoubleTap()
        #expect(!tap(&d, at: 0))
        #expect(tap(&d, at: 0.2))
    }

    @Test func tooSlowDoesNot() {
        var d = DoubleTap()
        #expect(!tap(&d, at: 0))
        #expect(!tap(&d, at: 0.6))
        #expect(tap(&d, at: 0.8), "the slow second tap counts as a new first one")
    }

    @Test func holdingIsNotTapping() {
        var d = DoubleTap()
        #expect(!tap(&d, at: 0, hold: 0.5))
        #expect(!tap(&d, at: 0.6))
    }

    @Test func aKeyOrModifierInBetweenStartsOver() {
        var d = DoubleTap()
        #expect(!tap(&d, at: 0))
        d.key()
        #expect(!tap(&d, at: 0.2))
        var e = DoubleTap()
        #expect(!tap(&e, at: 0))
        _ = e.modifiers(key: true, others: true, at: 0.15) // Control-Shift
        #expect(!tap(&e, at: 0.2))
    }

    @Test func threeTapsFireOnce() {
        var d = DoubleTap()
        #expect(!tap(&d, at: 0))
        #expect(tap(&d, at: 0.2))
        #expect(!tap(&d, at: 0.4))
    }

    @Test func defaultIsDoubleControlAndRoundTrips() {
        let d = UserDefaults(suiteName: "PanelShortcutTests-\(UUID().uuidString)")!
        #expect(PanelShortcut.load(d) == .doubleTap(.control))
        let k = HotKey(keyCode: 46, key: "m", modifiers: [.control, .option])
        PanelShortcut.save(.combo(k), d)
        #expect(PanelShortcut.load(d) == .combo(k))
        PanelShortcut.save(.none, d)
        #expect(PanelShortcut.load(d) == .none)
        PanelShortcut.save(.doubleTap(.option), d)
        #expect(PanelShortcut.load(d) == .doubleTap(.option))
        #expect(PanelShortcut.doubleTap(.option).display == "⌥ ⌥")
    }
}
