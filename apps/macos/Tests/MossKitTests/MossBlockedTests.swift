import Foundation
import Testing
@testable import MossKit

/// F59: "Open Moss's window" opens Moss's own tmux session, not a session
/// that happens to be called moss.
@Suite struct MossBlockedTests {
    @Test func theButtonRunsMossAttach() {
        let cli = "/Applications/cc moss/cc-moss"
        let command = MossBlockedNotice.command(cli: cli)
        #expect(command == "'/Applications/cc moss/cc-moss' 'moss' 'attach'")
        #expect(command != LaunchPlan.attachCommand(cli: cli, handle: "moss"))
    }

    @Test func theCommandRunsInTheChosenTerminal() throws {
        let terminal = InstalledTerminal(terminal: .ghostty, appPath: "/Applications/Ghostty.app")
        let plan = try LaunchPlan.make(for: terminal, command: MossBlockedNotice.command(cli: "/bin/cc-moss"),
                                       shell: "/bin/zsh")
        guard case .process(_, let arguments) = plan else {
            Issue.record("expected a process plan, got \(plan)")
            return
        }
        #expect(arguments.contains { $0.contains("'/bin/cc-moss' 'moss' 'attach'") }, "\(arguments)")
    }
}
