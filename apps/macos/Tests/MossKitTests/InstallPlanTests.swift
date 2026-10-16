import Foundation
import Testing
@testable import MossKit

@Suite struct InstallPlanTests {
    private let json = """
    {"schema":1,"parts":[
      {"part":"app","state":"ready"},
      {"part":"hooks","state":"changes","summary":"add 5 hooks"},
      {"part":"tmux","state":"ready"},
      {"part":"starship","state":"skipped","summary":"not installed"},
      {"part":"shell","state":"ready"},
      {"part":"aliases","state":"skipped"}
    ]}
    """

    @Test func decodesAndOrdersWhatTheWelcomeShows() throws {
        let plan = try InstallPlan.decode(Data(json.utf8))
        #expect(plan.visible.map(\.part) == ["hooks", "tmux", "starship", "shell"])
        #expect(plan.visible.first?.summary == "add 5 hooks")
        #expect(!plan.isReady)
    }

    @Test func readyWhenNothingChanges() throws {
        let ready = json.replacingOccurrences(of: "\"changes\"", with: "\"ready\"")
        #expect(try InstallPlan.decode(Data(ready.utf8)).isReady)
    }

    @Test func newerSchemaIsRefused() {
        #expect(throws: (any Error).self) { try InstallPlan.decode(Data(#"{"schema":2,"parts":[]}"#.utf8)) }
    }
}
