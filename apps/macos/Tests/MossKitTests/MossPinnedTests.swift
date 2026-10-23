import Foundation
import Testing
@testable import MossKit

private func moss(id: String, status: String = "running", lastActive: String = "2026-10-16T10:00:00.000Z") -> [String: Any] {
    var s = sessionJSON(id: id, name: "moss", status: status, cwd: "/home/u/.local/state/cc-moss/moss", lastActive: lastActive)
    s["role"] = "moss"
    return s
}

@Suite struct MossPinnedTests {
    let list = sessions([
        moss(id: "M1", status: "closed", lastActive: "2026-10-16T08:00:00.000Z"),
        moss(id: "M2", status: "interrupted", lastActive: "2026-10-16T09:00:00.000Z"),
        moss(id: "M3"),
        sessionJSON(id: "A", name: "api"),
        sessionJSON(id: "B", name: "web", status: "interrupted"),
    ])

    @Test func theRoleIsReadAndOptional() {
        #expect(list[2].isMoss)
        #expect(list[2].role == "moss")
        #expect(!list[3].isMoss)
        #expect(list[3].role == nil)
        var unnamed = list[2]
        unnamed.name = nil
        #expect(unnamed.title == "Moss", "not its tmux name")
    }

    @Test func mossIsPinnedApartFromTheRest() {
        let split = Grouping.splitMoss(list)
        #expect(split.moss?.id == "M3", "the running one")
        #expect(split.others.map(\.id) == ["A", "B"], "old Moss records are nowhere")
        let projects = Grouping.projects(list, home: "/home/u")
        #expect(!projects.flatMap(\.sessions).contains { $0.isMoss })
    }

    @Test func theSessionListNeverHasMoss() {
        // The Sessions page lists your pins, then the folders, from the
        // sessions without Moss (user 2026-10-19: Moss has its own page).
        let others = Grouping.splitMoss(list).others
        let pins = Pins(["M3", "A"]).split(others)
        #expect(pins.pinned.map(\.id) == ["A"], "pinning Moss's id does nothing")
        let listed = pins.pinned + Grouping.projects(pins.rest, home: "/home/u").flatMap(\.sessions)
        #expect(!listed.contains { $0.isMoss })
        // A fixed clock: the fixtures' dates must not age into "older" (failed on 2026-10-23).
        let now = ISO8601DateFormatter().date(from: "2026-10-16T12:00:00Z")!
        #expect(StateCounts(others, now: now).interrupted == 1, "Moss never counts")
    }

    @Test func noRunningMossNoPinnedRow() {
        let split = Grouping.splitMoss(Array(list[0...1]) + [list[3]])
        #expect(split.moss == nil)
        #expect(split.others.map(\.id) == ["A"])
    }

    @Test func mossOffersNoStopReminding() {
        for s in list where s.isMoss {
            for hopeless in [false, true] {
                let every = RowActions(s, hopeless: hopeless).all.joined()
                #expect(!every.contains(.close), "\(s.id)")
                #expect(!every.contains(.stopReminding), "\(s.id)")
                #expect(!every.contains(.bringBack), "\(s.id)")
            }
            #expect(!Close.allowed(s))
        }
        let pinned = RowActions(list[2], hopeless: false)
        #expect(pinned.primary == .open)
        #expect(!pinned.all.joined().contains(.rename), "its name lives in its own files")
        #expect(pinned.inline.contains(.message))
    }

    @Test func onButNotRunningSaysWhy() throws {
        let noDaemon = try MossStatus.decode(Data(
            #"{"schema":1,"enabled":true,"running":false,"restarts":0,"daemon_running":false}"#.utf8))
        #expect(noDaemon.line(lang: .en) == "Moss · waiting for the daemon")
        #expect(noDaemon.line(lang: .zh) == "Moss · 在等守护进程")
        #expect(noDaemon.problem(lang: .en)?.contains("Start it below") == true)
        let failing = try MossStatus.decode(Data(
            #"{"schema":1,"enabled":true,"running":false,"restarts":0,"daemon_running":true,"last_error":"tmux is not on the PATH"}"#.utf8))
        #expect(failing.line(lang: .en) == "Moss · couldn't start")
        #expect(failing.line(lang: .zh) == "Moss · 没能启动")
        #expect(failing.problem(lang: .en) == "Last try: tmux is not on the PATH")
        let starting = try MossStatus.decode(Data(
            #"{"schema":1,"enabled":true,"running":false,"restarts":0,"daemon_running":true}"#.utf8))
        #expect(starting.line(lang: .en) == "Moss · starting…")
        #expect(starting.problem(lang: .en) == nil)
    }

    @Test func mossThatNeedsYouReachesTheIcon() throws {
        let split = Grouping.splitMoss(list)
        #expect(Grouping.alerting(split.others, moss: split.moss).map(\.id) == ["A", "B"],
                "a calm Moss is not counted")
        var asking = try #require(split.moss)
        asking.attention = "permission"
        let shown = Grouping.alerting(split.others, moss: asking)
        #expect(shown.map(\.id) == ["A", "B", "M3"])
        #expect(IconState(shown) == .needsYou)
        #expect(IconBadge.count(shown) == 1)
    }

    @Test func statusDecodesAndReads() throws {
        let json = #"{"schema":1,"enabled":true,"running":true,"since":"2026-10-18T08:00:00Z","restarts":1,"session_id":"abc"}"#
        let s = try MossStatus.decode(Data(json.utf8))
        #expect(s.enabled && s.running && s.restarts == 1 && s.sessionId == "abc")
        let start = try #require(parseTimestamp("2026-10-18T08:00:00Z"))
        let later = start.addingTimeInterval(2 * 3600 + 60)
        #expect(s.line(now: later, lang: .en) == "Moss · running for 2 hours (restarted once)")
        #expect(s.line(now: later, lang: .zh) == "Moss · 运行中 2 小时（重启过 1 次）")
        var calm = s
        calm.restarts = 0
        #expect(calm.line(now: later, lang: .en) == "Moss · running for 2 hours")
        calm.restarts = 3
        #expect(calm.line(now: later, lang: .en) == "Moss · running for 2 hours (restarted 3 times)")
        let off = try MossStatus.decode(Data(#"{"schema":1,"enabled":false,"running":false,"restarts":0}"#.utf8))
        #expect(off.line(lang: .en) == "Moss · off")
        #expect(off.line(lang: .zh) == "Moss · 已关闭")
        #expect(off.problem(lang: .en) == nil, "off is a choice")
        #expect(s.daemonRunning, "an older cc-moss does not say: assume it runs")
        #expect(throws: MossClient.Failure.newerSchema(2)) {
            try MossStatus.decode(Data(#"{"schema":2,"enabled":true,"running":true}"#.utf8))
        }
    }
}
