import Foundation
import Testing
@testable import MossKit

@Suite struct CloseTests {
    @Test func buildsCommand() async throws {
        #expect(Close.arguments(targets: ["job-a", "-odd"]) == ["close", "--", "job-a", "-odd"])
        let runner = RecordingRunner([])
        try await MossClient(executable: "/x/cc-moss", runner: runner).close(["job-a"])
        #expect(await runner.calls == [["close", "--", "job-a"]])
    }
}
