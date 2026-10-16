import Foundation
import Testing
@testable import MossKit

@Suite struct HotKeyTests {
    @Test func displaysInMacOrder() {
        // ⌃ ⌥ ⇧ ⌘, then the key, as menus show it.
        let k = HotKey(keyCode: 46, key: "m", modifiers: [.command, .shift, .option, .control])
        #expect(k.display == "⌃⌥⇧⌘M")
        #expect(HotKey.default.display == "⌃⌥M")
    }

    @Test func needsAModifier() {
        #expect(!HotKey(keyCode: 46, key: "m", modifiers: []).isValid)
        #expect(!HotKey(keyCode: 46, key: "m", modifiers: [.shift]).isValid)
        #expect(HotKey(keyCode: 46, key: "m", modifiers: [.option]).isValid)
    }

    @Test func roundTripsThroughDefaults() {
        let d = UserDefaults(suiteName: "HotKeyTests-\(UUID().uuidString)")!
        #expect(HotKey.load(d) == HotKey.default, "unset → the default")
        let k = HotKey(keyCode: 49, key: " ", modifiers: [.control, .option])
        HotKey.save(k, d)
        #expect(HotKey.load(d) == k)
        HotKey.save(nil, d)
        #expect(HotKey.load(d) == nil, "cleared stays cleared")
    }

    @Test func spaceHasAName() {
        #expect(HotKey(keyCode: 49, key: " ", modifiers: [.option]).display == "⌥Space")
    }
}
