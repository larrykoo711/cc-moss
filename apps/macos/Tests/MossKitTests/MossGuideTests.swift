import Foundation
import Testing
@testable import MossKit

@Suite struct MossGuideTests {
    @Test func theRailPointsAtMossUntilItsPageIsOpened() {
        #expect(MossGuide.hints(mossOn: true, seen: false))
        #expect(!MossGuide.hints(mossOn: true, seen: true))
        #expect(!MossGuide.hints(mossOn: false, seen: false), "switched off: nothing to point at")
    }

    @Test func seenIsRememberedForGood() {
        let defaults = scratchDefaults("MossGuideTests")
        #expect(!MossGuide.seen(defaults))
        MossGuide.markSeen(defaults)
        #expect(MossGuide.seen(defaults))
    }

    @Test func theQuickBarIsExplainedUntilItIsUsed() {
        #expect(MossGuide.explainsQuickBar(uses: []))
        #expect(!MossGuide.explainsQuickBar(uses: [MossQuickUse(id: "cmd:/usage", at: Date())]))
    }

    @Test func statusCarriesMossFolder() throws {
        let with = try MossStatus.decode(Data(#"{"schema":1,"enabled":true,"running":true,"home":"/tmp/state/moss"}"#.utf8))
        #expect(with.home == "/tmp/state/moss")
        // An older cc-moss doesn't say; an empty one is no folder.
        let old = try MossStatus.decode(Data(#"{"schema":1,"enabled":true,"running":true}"#.utf8))
        #expect(old.home == nil)
        let empty = try MossStatus.decode(Data(#"{"schema":1,"enabled":true,"running":true,"home":""}"#.utf8))
        #expect(empty.home == nil)
    }
}
