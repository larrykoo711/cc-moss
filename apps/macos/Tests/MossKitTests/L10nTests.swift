import Foundation
import Testing
@testable import MossKit

@Suite struct L10nTests {
    var utc: Calendar {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: "UTC")!
        return c
    }
    let now = parseTimestamp("2026-10-16T12:00:00Z")!

    @Test func detectsLanguage() {
        #expect(Lang.detect(env: ["CC_MOSS_LANG": "zh_CN.UTF-8"], preferred: ["en-US"]) == .zh)
        #expect(Lang.detect(env: ["CC_MOSS_LANG": "auto"], preferred: ["zh-Hans-CN"]) == .zh)
        #expect(Lang.detect(env: [:], preferred: ["en-GB", "zh-Hans"]) == .en)
        #expect(Lang.detect(env: [:], preferred: []) == .en)
    }

    @Test func agoInEnglish() {
        L.$override.withValue(.en) { agoInEnglishBody() }
    }

    private func agoInEnglishBody() {
        let ago = { (s: Double) in L.ago(self.now.addingTimeInterval(-s), now: self.now, calendar: self.utc) }
        #expect(ago(10) == "just now")
        #expect(ago(60) == "1 minute ago")
        #expect(ago(300) == "5 minutes ago")
        #expect(ago(3 * 3600) == "3 hours ago")
        #expect(ago(14 * 3600) == "yesterday")
        #expect(ago(5 * 86_400) == "Oct 11")
    }

    @Test func agoInChinese() {
        L.$override.withValue(.zh) { agoInChineseBody() }
    }

    private func agoInChineseBody() {
        let ago = { (s: Double) in L.ago(self.now.addingTimeInterval(-s), now: self.now, calendar: self.utc) }
        #expect(ago(10) == "刚刚")
        #expect(ago(300) == "5 分钟前")
        #expect(ago(3 * 3600) == "3 小时前")
        #expect(ago(14 * 3600) == "昨天")
        #expect(ago(5 * 86_400) == "10月11日")
    }
}
