import Testing
@testable import MossKit

@Suite struct VoicePickerTests {
    private func v(_ id: String, _ language: String, _ quality: VoiceQuality = .standard, novelty: Bool = false) -> VoiceInfo {
        VoiceInfo(id: id, language: language, name: id, quality: quality, novelty: novelty)
    }

    @Test func filtersByLanguage() {
        let all = [v("a", "en-US"), v("b", "zh-CN"), v("c", "fr-FR"), v("d", "en-GB")]
        #expect(VoicePicker.installed(all, for: .en).map(\.id).sorted() == ["a", "d"])
        #expect(VoicePicker.installed(all, for: .zh).map(\.id) == ["b"])
    }

    @Test func skipsNovelty() {
        let all = [v("bells", "en-US", novelty: true), v("plain", "en-US")]
        #expect(VoicePicker.installed(all, for: .en).map(\.id) == ["plain"])
    }

    @Test func premiumBeatsEnhancedBeatsDefault() {
        let all = [v("std", "en-US"), v("enh", "en-US", .enhanced), v("pre", "en-US", .premium)]
        #expect(VoicePicker.installed(all, for: .en).map(\.id) == ["pre", "enh", "std"])
        #expect(VoicePicker.choose(stored: nil, from: all, for: .en)?.id == "pre")
    }

    @Test func mandarinBeforeCantonese() {
        let all = [v("hk", "zh-HK", .premium), v("tw", "zh-TW", .enhanced), v("cn", "zh-CN")]
        #expect(VoicePicker.installed(all, for: .zh).map(\.id) == ["tw", "cn", "hk"])
    }

    @Test func storedStillInstalledWins() {
        let all = [v("std", "en-US"), v("pre", "en-US", .premium)]
        #expect(VoicePicker.choose(stored: "std", from: all, for: .en)?.id == "std")
    }

    @Test func storedButGoneFallsBack() {
        let all = [v("std", "en-US"), v("enh", "en-US", .enhanced)]
        #expect(VoicePicker.choose(stored: "deleted-premium", from: all, for: .en)?.id == "enh")
    }

    @Test func storedFromOtherLanguageFallsBack() {
        // The interface switched to English with a Chinese voice still stored: don't read English with it.
        let all = [v("cn", "zh-CN", .premium), v("us", "en-US")]
        #expect(VoicePicker.choose(stored: "cn", from: all, for: .en)?.id == "us")
    }

    @Test func noneInstalledIsNil() {
        #expect(VoicePicker.choose(stored: nil, from: [v("fr", "fr-FR")], for: .zh) == nil)
    }
}
