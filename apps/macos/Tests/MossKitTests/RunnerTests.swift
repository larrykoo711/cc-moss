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
}
