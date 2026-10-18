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

    @Test func thePinnedRowFollowsTheSearch() {
        let m = list[2]
        #expect(Filter.keeps(m, label: "Moss", query: ""))
        #expect(Filter.keeps(m, label: "Moss", query: "mo"))
        #expect(!Filter.keeps(m, label: "Moss", query: "billing"))
        #expect(!Filter.keeps(m, label: "Moss", query: "", state: .interrupted))
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
        #expect(throws: MossClient.Failure.newerSchema(2)) {
            try MossStatus.decode(Data(#"{"schema":2,"enabled":true,"running":true}"#.utf8))
        }
    }
}
