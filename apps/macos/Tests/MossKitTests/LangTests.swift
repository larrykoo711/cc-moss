import Foundation
import Testing
@testable import MossKit

@Suite struct LangTests {
    @Test func configBeatsSystem() {
        #expect(Lang.detect(env: [:], config: "zh", preferred: ["en-US"]) == .zh)
        #expect(Lang.detect(env: [:], config: "en", preferred: ["zh-Hans-CN"]) == .en)
    }

    @Test func autoFollowsSystem() {
        #expect(Lang.detect(env: [:], config: "auto", preferred: ["zh-Hans-CN"]) == .zh)
        #expect(Lang.detect(env: [:], config: nil, preferred: ["en-GB"]) == .en)
    }

    @Test func unknownIsIgnored() {
        #expect(Lang.detect(env: [:], config: "klingon", preferred: ["zh-Hans-CN"]) == .zh)
    }

    @Test func envStillWins() {
        #expect(Lang.detect(env: ["CC_MOSS_LANG": "en"], config: "zh", preferred: ["zh-Hans"]) == .en)
    }

    @Test func choiceRoundTrips() {
        #expect(LangChoice(config: "zh") == .zh)
        #expect(LangChoice(config: nil) == .auto)
        #expect(LangChoice(config: "weird") == .auto)
        #expect(LangChoice.en.configValue == "en")
    }
}
