import Foundation
import Testing
@testable import MossKit

@Suite struct PermissionModeTests {
    private func reply(_ value: String) -> CommandResult {
        CommandResult(status: 0, stdout: Data(#"{"schema":1,"ok":true,"key":"claude.permission_mode","value":"\#(value)"}"#.utf8), stderr: "")
    }

    @Test func readsTheConfigLikeTheCLI() {
        #expect(PermissionMode(config: "bypass") == .bypass)
        #expect(PermissionMode(config: " Default\n") == .default)
        #expect(PermissionMode(config: nil) == .bypass)
        #expect(PermissionMode(config: "weird") == .bypass)
        #expect(PermissionMode(bypass: false).configValue == "default")
        #expect(PermissionMode(bypass: true).isBypass)
    }

    @Test func getsThroughConfigGet() async throws {
        let runner = RecordingRunner([reply("default")])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        #expect(try await client.permissionMode() == .default)
        #expect(await runner.calls == [["config", "get", "claude.permission_mode", "--json"]])
    }

    @Test func setsThroughConfigSet() async throws {
        let runner = RecordingRunner([reply("default")])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        try await client.setPermissionMode(.default)
        #expect(await runner.calls == [["config", "set", "claude.permission_mode", "default", "--json"]])
    }

    @Test func aRefusalIsAFailure() async {
        let runner = RecordingRunner([CommandResult(status: 1, stdout: Data(), stderr: "cc-moss: no\n")])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        await #expect(throws: MossClient.Failure.self) {
            try await client.setPermissionMode(.bypass)
        }
    }
}
