import Foundation
import Testing
@testable import MossKit

/// F59: the Moss page while Moss is stuck on a choice in its window.
@Suite struct MossPageTests {
    private func status(_ json: String) throws -> MossStatus {
        try MossStatus.decode(Data(json.utf8))
    }

    private let blockedJSON = #"{"schema":1,"enabled":true,"running":true,"activity":"blocked","blocked":{"kind":"bypass_warning","since":"2026-11-09T10:00:00Z"},"queued":2}"#

    @Test func decodesWhatMossIsStuckOn() throws {
        let s = try status(blockedJSON)
        #expect(s.activity == .blocked)
        #expect(s.blocked == MossBlocked(kind: .bypassWarning, since: parseTimestamp("2026-11-09T10:00:00Z")))
        #expect(s.activity.canTalk, "messages still wait their turn")
        let login = try status(#"{"schema":1,"enabled":true,"running":true,"activity":"blocked","blocked":{"kind":"login","since":"2026-11-09T10:00:00Z"}}"#)
        #expect(login.blocked?.kind == .login)
        // A kind a newer cc-moss knows: still stuck, said plainly.
        let newer = try status(#"{"schema":1,"enabled":true,"running":true,"activity":"blocked","blocked":{"kind":"theme_picker"}}"#)
        #expect(newer.blocked?.kind == .unknownDialog)
        #expect(try status(#"{"schema":1,"enabled":true,"running":true,"activity":"idle"}"#).blocked == nil)
    }

    @Test func queuedMessagesSayTheyWaitOnTheQuestion() throws {
        let s = try status(blockedJSON)
        #expect(s.queuedLabel(lang: .en) == "Waiting for Moss to get past a question")
        #expect(s.queuedLabel(lang: .zh) == "等 Moss 处理一个确认框")
        #expect(s.queuedLabel(lang: .en) != "Sending…")
    }

    @Test func otherStatesKeepTheirLabels() {
        let idle = MossStatus(enabled: true, running: true, activity: .idle)
        #expect(idle.queuedLabel(lang: .en) == "Sending…")
        #expect(idle.queuedLabel(lang: .zh) == "发送中…")
        let busy = MossStatus(enabled: true, running: true, activity: .thinking)
        #expect(busy.queuedLabel(lang: .en) == "In line")
        #expect(busy.queuedLabel(lang: .zh) == "排队中")
        #expect(idle.blockedNotice(lang: .en) == nil)
        #expect(busy.blockedNotice(lang: .en) == nil)
    }

    @Test func theNoticeSaysWhereItStoppedAndOffersItsWindow() throws {
        let s = try status(blockedJSON)
        let en = try #require(s.blockedNotice(lang: .en))
        #expect(en.title == "Moss stopped on Claude Code's warning about skipping permissions")
        #expect(en.detail.contains("Yes, I accept"))
        #expect(en.button == "Open Moss's window")
        let zh = try #require(s.blockedNotice(lang: .zh))
        #expect(zh.title == "Moss 停在了 Claude Code 跳过权限确认的警告上")
        #expect(zh.button == "打开 Moss 的窗口")
        for text in [en.title, en.detail, zh.title, zh.detail] {
            #expect(!text.contains("!") && !text.contains("！"), "\(text)")
            #expect(!text.contains("bypass_warning"), "no codes: \(text)")
        }
        let login = MossStatus(enabled: true, running: true, activity: .blocked,
                               blocked: MossBlocked(kind: .login, since: nil))
        #expect(login.blockedNotice(lang: .en)?.title == "Moss's Claude Code wants you to sign in")
        let trust = MossStatus(enabled: true, running: true, activity: .blocked,
                               blocked: MossBlocked(kind: .trustFolder, since: nil))
        #expect(trust.blockedNotice(lang: .en)?.title == "Claude Code asks whether to trust Moss's folder")
        #expect(trust.blockedNotice(lang: .en)?.detail.contains("Yes, I trust this folder") == true)
        #expect(trust.blockedNotice(lang: .zh)?.title == "Claude Code 在问要不要信任 Moss 的文件夹")
        let decoded = try MossStatus.decode(Data(#"{"schema":1,"enabled":true,"running":true,"activity":"blocked","blocked":{"kind":"trust_folder"}}"#.utf8))
        #expect(decoded.blocked?.kind == .trustFolder)
        let other = MossStatus(enabled: true, running: true, activity: .blocked,
                               blocked: MossBlocked(kind: .unknownDialog, since: nil))
        #expect(other.blockedNotice(lang: .en)?.title == "Moss stopped on a question in its window")
    }

    @Test func theStatusLineAndTheThinkingBubble() throws {
        let s = try status(blockedJSON)
        #expect(s.activityLine(lang: .en) == "Stopped on a question in its window · 2 in line")
        #expect(s.activityLine(lang: .zh) == "停在它窗口里的一个确认框上 · 还排着 2 条")
        let mine = MossMessage(id: "m1", at: Date(), from: .you, text: "hi", delivery: .queued)
        #expect(MossWaiting.of([mine], activity: .blocked)?.queued == true)
    }
}
