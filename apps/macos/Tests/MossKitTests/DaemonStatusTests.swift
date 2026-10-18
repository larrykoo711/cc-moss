import Foundation
import Testing
@testable import MossKit

@Suite struct DaemonStatusTests {
    @Test func decodesARunningDaemon() throws {
        let json = #"{"schema":1,"running":true,"pid":4242,"version":"0.2.2","started_at":"2026-10-18T08:00:00Z","heartbeat_at":"2026-10-18T11:00:00.5Z"}"#
        let s = try DaemonStatus.decode(Data(json.utf8))
        #expect(s.running)
        #expect(s.pid == 4242)
        #expect(s.version == "0.2.2")
        #expect(s.startedAt == parseTimestamp("2026-10-18T08:00:00Z"))
        #expect(s.heartbeatAt != nil)
    }

    @Test func decodesAStoppedDaemon() throws {
        let s = try DaemonStatus.decode(Data(#"{"schema":1,"running":false}"#.utf8))
        #expect(s == .stopped)
    }

    @Test func refusesANewerSchema() {
        #expect(throws: MossClient.Failure.newerSchema(2)) {
            try DaemonStatus.decode(Data(#"{"schema":2,"running":true}"#.utf8))
        }
    }

    @Test func statusLineSaysHowLong() throws {
        let start = try #require(parseTimestamp("2026-10-18T08:00:00Z"))
        let running = DaemonStatus(running: true, pid: 1, version: "0.2.2", startedAt: start)
        #expect(running.line(now: start.addingTimeInterval(3 * 3600 + 120), lang: .en) == "Daemon · running for 3 hours")
        #expect(running.line(now: start.addingTimeInterval(60), lang: .en) == "Daemon · running for 1 minute")
        #expect(running.line(now: start.addingTimeInterval(5), lang: .en) == "Daemon · running for less than a minute")
        #expect(DaemonStatus.stopped.line(lang: .en) == "Daemon · not running")
        #expect(running.line(now: start.addingTimeInterval(3 * 3600), lang: .zh) == "守护进程 · 运行中 3 小时")
        #expect(DaemonStatus.stopped.line(lang: .zh) == "守护进程 · 没在运行")
    }

    @Test func clientAsksTheCLI() async throws {
        let client = MossClient(executable: "/x/cc-moss",
                                runner: CannedRunner(stdout: Data(#"{"schema":1,"running":false}"#.utf8)))
        #expect(try await client.daemonStatus() == .stopped)
    }
}
