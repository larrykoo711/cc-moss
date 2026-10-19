import AppKit
import Foundation
import Testing
@testable import MossKit

@Suite struct AppearanceChoiceTests {
    private func defaults() -> UserDefaults {
        let name = "cc-moss-appearance-tests-\(UUID().uuidString)"
        let d = UserDefaults(suiteName: name)!
        d.removePersistentDomain(forName: name)
        return d
    }

    @Test func nothingStoredFollowsTheMac() {
        #expect(AppearanceChoice.load(defaults()) == .system)
    }

    @Test func aChoiceIsKept() {
        let d = defaults()
        AppearanceChoice.dark.save(d)
        #expect(AppearanceChoice.load(d) == .dark)
        AppearanceChoice.system.save(d)
        #expect(d.object(forKey: AppearanceChoice.key) == nil, "back to the default: nothing stored")
        #expect(AppearanceChoice.load(d) == .system)
    }

    @Test func somethingUnknownFollowsTheMac() {
        let d = defaults()
        d.set("sepia", forKey: AppearanceChoice.key)
        #expect(AppearanceChoice.load(d) == .system)
    }

    @Test func eachChoiceMapsToAnAppearance() {
        #expect(AppearanceChoice.system.appearanceName == nil)
        #expect(AppearanceChoice.light.appearanceName == NSAppearance.Name.aqua.rawValue)
        #expect(AppearanceChoice.dark.appearanceName == NSAppearance.Name.darkAqua.rawValue)
    }
}
