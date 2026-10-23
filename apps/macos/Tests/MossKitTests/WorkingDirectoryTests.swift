import Foundation
import Testing
@testable import MossKit

@Suite struct WorkingDirectoryTests {
    @Test func staleSelectionAndCancelledPickerCannotApprovePaths() throws {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let check = try decoder.decode(WorkingDirectoryCheck.self, from: Data(#"{"path":"/a","ok":true,"canonical":"/real/a","source_resolved":true}"#.utf8))
        var selection = WorkingDirectorySelection()
        let old = selection.begin("/a")
        _ = selection.begin("/b")
        let acceptedOld = selection.complete(old, check: check)
        #expect(!acceptedOld && selection.canonical == nil)
        let current = selection.begin("/a")
        selection.cancel()
        let acceptedCancelled = selection.complete(current, check: check)
        #expect(!acceptedCancelled && selection.canonical == nil)
        let next = selection.begin("/a")
        let accepted = selection.complete(next, check: check)
        #expect(accepted && selection.canonical == "/real/a")
    }

    @Test(arguments: ["cwd_forbidden", "cwd_missing", "cwd_not_directory", "cwd_unreadable", "cwd_unavailable", "cwd_invalid", "cwd_changed"])
    func directoryErrorsNeverOfferSettingsReplacement(_ raw: String) throws {
        let reason = try #require(RestoreReason(rawValue: raw))
        #expect(!reason.isLocationProblem)
        L.$override.withValue(.zh) { #expect(reason.explanation.contains("工作目录")) }
    }

    @Test func missingAndMalformedCapabilitiesDoNotHideSessionsOrAuthorizeChecks() throws {
        for value in ["null", "\"bad\"", "{\"working_directory\":true,\"new_receipt\":2}"] {
            let data = Data("{\"schema\":1,\"sessions\":[],\"capabilities\":\(value)}".utf8)
            let snapshot = try Snapshot.decode(data)
            #expect(!snapshot.capabilities.checksWorkingDirectory && !snapshot.capabilities.returnsNewReceipt)
        }
    }

    @Test func oldCLIReceivesNoNewPolicyCommand() async {
        let runner = Recorder(stdout: "{}")
        let client = MossClient(executable: "/demo/cc-moss", runner: runner)
        await #expect(throws: MossClient.Failure.command(WorkingDirectoryPolicy.updateRequired)) {
            _ = try await client.checkDirectories(["/demo/project"], capabilities: .init())
        }
        #expect(await runner.calls.isEmpty)
    }

    @Test func checksAreBoundToExactInputAndCanonicalOutput() async throws {
        let runner = Recorder(stdout: #"{"schema":1,"results":[{"path":"/demo/a b","ok":true,"canonical":"/real/project","reason":null,"message":null,"source_resolved":true}]}"#)
        let result = try await MossClient(executable: "/demo/cc-moss", runner: runner)
            .checkDirectories(["/demo/a b"], capabilities: .init(workingDirectory: 1))
        #expect(await runner.calls == [["check-directory", "--json", "--", "/demo/a b"]])
        #expect(result.first?.approvedForNewSession == true && result.first?.canonical == "/real/project")
    }

    @Test func mismatchedResponsesCannotApproveAnotherDirectory() async {
        let runner = Recorder(stdout: #"{"schema":1,"results":[{"path":"/other","ok":true,"canonical":"/other","source_resolved":true}]}"#)
        await #expect(throws: MossClient.Failure.self) {
            _ = try await MossClient(executable: "/demo/cc-moss", runner: runner)
                .checkDirectories(["/requested"], capabilities: .init(workingDirectory: 1))
        }
    }

    @Test func baseUnknownContextIsNotNewSessionApproval() throws {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let check = try decoder.decode(WorkingDirectoryCheck.self, from: Data(#"{"ok":true,"canonical":"/demo/project","source_resolved":false}"#.utf8))
        #expect(!check.approvedForNewSession)
    }

    @Test func directoryFailureRemainsSeparateFromSettingsSource() {
        let notice = FailureNotice(action: .restore, subject: nil, reason: "cwd_forbidden")
        L.$override.withValue(.zh) { #expect(notice.why.contains("工作目录")) }
    }
}
