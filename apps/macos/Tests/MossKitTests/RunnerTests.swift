import Foundation
import Testing
@testable import MossKit

@Suite struct RunnerTests {
    @Test func augmentsPath() {
        let path = augmentedPath(for: "/home/u/.cargo/bin/cc-moss", current: "/usr/bin:/bin")
        #expect(path == "/home/u/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
    }

    @Test func capturesOutputAndStatus() async throws {
        let r = try await ProcessRunner().run("/bin/sh", ["-c", "echo out; echo err >&2; exit 3"], timeout: 5)
        #expect(r.status == 3)
        #expect(String(decoding: r.stdout, as: UTF8.self) == "out\n")
        #expect(r.stderr == "err\n")
    }

    @Test func passesAugmentedPathToTheChild() async throws {
        let r = try await ProcessRunner(environment: ["PATH": "/usr/bin:/bin"]).run("/bin/sh", ["-c", "echo $PATH"], timeout: 5)
        #expect(String(decoding: r.stdout, as: UTF8.self).hasPrefix("/bin:/opt/homebrew/bin:/usr/local/bin:"))
    }

    @Test func timesOut() async {
        let started = Date()
        await #expect(throws: RunError.timedOut(seconds: 0.5)) {
            _ = try await ProcessRunner().run("/bin/sleep", ["30"], timeout: 0.5)
        }
        #expect(Date().timeIntervalSince(started) < 5)
    }

    /// Leaving a settings page cancels its load while the command still runs:
    /// the child is stopped and the call ends with a cancellation, rather than
    /// asking a running process for its exit status (which aborts the app).
    @Test func cancellingStopsTheCommand() async {
        let started = Date()
        let run = Task { try await ProcessRunner().run("/bin/sleep", ["30"], timeout: 20) }
        try? await Task.sleep(nanoseconds: 200_000_000)
        run.cancel()
        await #expect(throws: CancellationError.self) { _ = try await run.value }
        #expect(Date().timeIntervalSince(started) < 5)
    }

    /// More slow commands at once than the machine has cores (CI has three):
    /// waiting on one must not hold a thread the others need to time out.
    @Test func manySlowCommandsAllTimeOut() async {
        let started = Date()
        await withTaskGroup(of: Bool.self) { group in
            for _ in 0..<(ProcessInfo.processInfo.activeProcessorCount + 2) {
                group.addTask {
                    do {
                        _ = try await ProcessRunner().run("/bin/sleep", ["30"], timeout: 0.5)
                        return false
                    } catch {
                        return error as? RunError == .timedOut(seconds: 0.5)
                    }
                }
            }
            for await timedOut in group { #expect(timedOut) }
        }
        #expect(Date().timeIntervalSince(started) < 5)
    }
}

/// Answers `ls --json` with canned output.
struct FakeRunner: CommandRunner {
    var result: CommandResult
    func run(_ executable: String, _ arguments: [String], timeout: Double) async throws -> CommandResult { result }
}

@Suite struct ClientTests {
    @Test func reportsTheFirstErrorLine() async {
        let client = MossClient(executable: "/x/cc-moss", runner: FakeRunner(result: .init(status: 1, stdout: Data(), stderr: "\ncc-moss: registry is locked\nmore\n")))
        await #expect(throws: MossClient.Failure.command("cc-moss: registry is locked")) {
            _ = try await client.snapshot()
        }
    }

    @Test func newerSchemaIsItsOwnFailure() async {
        let client = MossClient(executable: "/x/cc-moss", runner: FakeRunner(result: .init(status: 0, stdout: snapshotData([], schema: 3), stderr: "")))
        await #expect(throws: MossClient.Failure.newerSchema(3)) {
            _ = try await client.snapshot()
        }
    }

    @Test func versionIsTheLastWord() async throws {
        let client = MossClient(executable: "/x/cc-moss", runner: FakeRunner(result: .init(status: 0, stdout: Data("cc-moss 0.2.0\n".utf8), stderr: "")))
        #expect(try await client.version() == "0.2.0")
    }

    @Test func decodesUpdateInfo() throws {
        let json = #"{"schema":1,"current":"0.1.1","latest":"0.2.0","available":true,"notes":"https://example.com/tag/v0.2.0"}"#
        let info = try UpdateInfo.decode(Data(json.utf8))
        #expect(info.available && info.latest == "0.2.0" && info.notes != nil)
        #expect(throws: SnapshotError.newerSchema(2)) {
            try UpdateInfo.decode(Data(#"{"schema":2}"#.utf8))
        }
    }
}
