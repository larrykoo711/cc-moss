import Foundation
import Testing
@testable import MossKit

@Suite struct PermissionNeedsTests {
    private func installed(_ t: Terminal, _ version: String? = nil) -> InstalledTerminal {
        InstalledTerminal(terminal: t, appPath: "/Applications/\(t.rawValue).app", version: version)
    }

    @Test func terminalTabsNeedSystemEventsToo() {
        #expect(PermissionNeeds.automation(for: installed(.terminal), style: .tab) == [.terminal(.terminal), .systemEvents])
        #expect(PermissionNeeds.automation(for: installed(.terminal), style: .window) == [.terminal(.terminal)])
    }

    @Test func scriptedTerminalsNeedAutomation() {
        #expect(PermissionNeeds.automation(for: installed(.iterm2), style: .tab) == [.terminal(.iterm2)])
        #expect(PermissionNeeds.automation(for: installed(.ghostty, "1.3.0"), style: .tab) == [.terminal(.ghostty)])
    }

    @Test func terminalsStartedFromTheCommandLineNeedNothing() {
        #expect(PermissionNeeds.automation(for: installed(.ghostty, "1.2.3"), style: .tab).isEmpty)
        for t in [Terminal.wezterm, .kitty, .alacritty] {
            #expect(PermissionNeeds.automation(for: installed(t), style: .tab).isEmpty)
        }
    }

    @Test func accessibilityIsNeededForADoubleTapOrTerminalTabs() {
        #expect(PermissionNeeds.accessibility(shortcut: .doubleTap(.control), terminal: .ghostty, style: .tab))
        #expect(PermissionNeeds.accessibility(shortcut: .none, terminal: .terminal, style: .tab))
        #expect(!PermissionNeeds.accessibility(shortcut: .none, terminal: .terminal, style: .window))
        #expect(!PermissionNeeds.accessibility(shortcut: .combo(.default), terminal: .iterm2, style: .tab))
    }
}

@Suite struct GrantTests {
    @Test func automationStatusCodes() {
        #expect(Grant(automationStatus: 0) == .allowed)
        #expect(Grant(automationStatus: -1743) == .denied)
        #expect(Grant(automationStatus: -1744) == .notAsked)
        // procNotFound: the app isn't running, so macOS can't say.
        #expect(Grant(automationStatus: -600) == .unknown)
        #expect(Grant(automationStatus: -50) == .unknown)
    }

    @Test func attentionCountsOnlyWhatIsTurnedOff() {
        #expect(PermissionSummary.attention(accessibility: .denied, accessibilityNeeded: true,
                                            notifications: .denied, automation: [.denied, .allowed, .unknown]) == 3)
        #expect(PermissionSummary.attention(accessibility: .denied, accessibilityNeeded: false,
                                            notifications: .notDetermined, automation: [.notAsked]) == 0)
        #expect(PermissionSummary.attention(accessibility: .allowed, accessibilityNeeded: true,
                                            notifications: .allowed, automation: []) == 0)
    }
}

@Suite struct AccessibilityMemoryTests {
    private func fresh() -> AccessibilityMemory {
        let name = "perm-test-\(UUID().uuidString)"
        return AccessibilityMemory(defaults: UserDefaults(suiteName: name)!)
    }

    @Test func neverAllowedIsNotLost() {
        let memory = fresh()
        #expect(!memory.lost(trusted: false, needed: true))
    }

    @Test func allowedThenGoneIsLost() {
        let memory = fresh()
        memory.record(trusted: true)
        #expect(memory.lost(trusted: false, needed: true))
        #expect(!memory.lost(trusted: true, needed: true))
    }

    @Test func notLostWhenNothingNeedsIt() {
        let memory = fresh()
        memory.record(trusted: true)
        #expect(!memory.lost(trusted: false, needed: false))
    }

    @Test func recordingAnUntrustedLaunchKeepsTheMemory() {
        let memory = fresh()
        memory.record(trusted: true)
        memory.record(trusted: false)
        #expect(memory.lost(trusted: false, needed: true))
    }

    @Test func dismissingForgetsUntilAllowedAgain() {
        let memory = fresh()
        memory.record(trusted: true)
        memory.dismiss()
        #expect(!memory.lost(trusted: false, needed: true))
        memory.record(trusted: true)
        #expect(memory.lost(trusted: false, needed: true))
    }
}
