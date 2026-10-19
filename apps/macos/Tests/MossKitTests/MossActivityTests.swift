import Foundation
import Testing
@testable import MossKit

@Suite struct MossActivityTests {
    let now = Date(timeIntervalSince1970: 1_760_608_800)
    var since: Date { now.addingTimeInterval(-2 * 3600 - 300) }

    private func status(_ activity: String?, extra: String = "") throws -> MossStatus {
        let field = activity.map { #","activity":"\#($0)""# } ?? ""
        let json = #"{"schema":1,"enabled":true,"running":true,"since":"2026-10-16T08:00:00Z"\#(field)\#(extra)}"#
        return try MossStatus.decode(Data(json.utf8))
    }

    @Test func decodesTheNewFields() throws {
        let s = try status("thinking", extra: #","queued":2,"last_reply_at":"2026-10-16T09:59:00Z""#)
        #expect(s.activity == .thinking)
        #expect(s.queued == 2)
        #expect(s.lastReplyAt == parseTimestamp("2026-10-16T09:59:00Z"))
    }

    @Test func anOlderCLIStillGivesAnActivity() throws {
        #expect(try status(nil).activity == .idle)
        #expect(try status(nil).queued == 0)
        let off = try MossStatus.decode(Data(#"{"schema":1,"enabled":false,"running":false}"#.utf8))
        #expect(off.activity == .off)
        let down = try MossStatus.decode(Data(#"{"schema":1,"enabled":true,"running":true,"daemon_running":false}"#.utf8))
        #expect(down.activity == .offline)
        let starting = try MossStatus.decode(Data(#"{"schema":1,"enabled":true,"running":false}"#.utf8))
        #expect(starting.activity == .offline)
    }

    @Test func anUnknownActivityFallsBackToTheRest() throws {
        #expect(try status("dreaming").activity == .idle)
    }

    @Test func idleSaysHowLongItHasRun() {
        let s = MossStatus(enabled: true, running: true, since: since, activity: .idle)
        #expect(s.activityLine(now: now, lang: .zh) == "待命中 · 已运行 2 小时")
        #expect(s.activityLine(now: now, lang: .en) == "Standing by · up 2 hours")
    }

    @Test func thinking() {
        let s = MossStatus(enabled: true, running: true, since: since, activity: .thinking)
        #expect(s.activityLine(now: now, lang: .zh) == "正在想…")
        #expect(s.activityLine(now: now, lang: .en) == "Thinking…")
    }

    @Test func thinkingWithALine() {
        let s = MossStatus(enabled: true, running: true, since: since, activity: .thinking, queued: 2)
        #expect(s.activityLine(now: now, lang: .zh) == "正在想… · 还排着 2 条")
        #expect(s.activityLine(now: now, lang: .en) == "Thinking… · 2 in line")
    }

    @Test func waiting() {
        let s = MossStatus(enabled: true, running: true, activity: .waiting)
        #expect(s.activityLine(now: now, lang: .zh) == "在等你确认，去终端看看")
        #expect(s.activityLine(now: now, lang: .en) == "Waiting for your OK, check the terminal")
    }

    @Test func offlineRestarting() {
        let s = MossStatus(enabled: true, running: false, activity: .offline)
        #expect(s.activityLine(now: now, lang: .zh) == "重启中，30 秒后再试")
        #expect(s.activityLine(now: now, lang: .en) == "Restarting, try again in 30 seconds")
        #expect(s.inputBlocked(lang: .zh) == "重启中，30 秒后再试")
    }

    @Test func offlineSaysWhy() {
        let failed = MossStatus(enabled: true, running: false, lastError: "Claude Code exited", activity: .offline)
        #expect(failed.activityLine(now: now, lang: .zh) == "没能启动：Claude Code exited")
        let down = MossStatus(enabled: true, running: false, daemonRunning: false, activity: .offline)
        #expect(down.activityLine(now: now, lang: .en) == "Not running in the background · start it in Settings")
    }

    @Test func off() {
        let s = MossStatus(enabled: false, running: false, activity: .off)
        #expect(s.activityLine(now: now, lang: .zh) == "已关闭 · 去设置里打开")
        #expect(s.activityLine(now: now, lang: .en) == "Off · turn it on in Settings")
        #expect(s.inputBlocked(lang: .zh) == "Moss 关着，去设置里打开就能聊。")
    }

    @Test func onlyOnAndRunningCanTalk() {
        #expect(MossActivity.allCases.filter(\.canTalk) == [.idle, .thinking, .waiting])
        #expect(MossStatus(enabled: true, running: true, activity: .thinking).inputBlocked(lang: .en) == nil)
    }
}
