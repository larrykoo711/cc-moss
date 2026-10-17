import Foundation
import Testing
@testable import MossKit

@Suite struct LaunchPlanTests {
    let cli = "/opt/homebrew/bin/cc-moss"
    let name = "it's \"登录页\""
    /// `'/opt/homebrew/bin/cc-moss' attach 'it'\''s "登录页"'`
    var cmd: String { LaunchPlan.attachCommand(cli: cli, handle: name) }
    /// The same, run by the login shell: `'/bin/zsh' '-lc' '<cmd quoted again>'`.
    var viaShell: String { ["/bin/zsh", "-lc", cmd].map(LaunchPlan.quoteAlways).joined(separator: " ") }

    func plan(_ t: Terminal, version: String? = nil, app: String = "/Applications/X.app") throws -> LaunchPlan {
        try LaunchPlan.make(for: InstalledTerminal(terminal: t, appPath: app, version: version),
                            cli: cli, handle: name, shell: "/bin/zsh")
    }

    @Test func commandShape() {
        #expect(cmd == #"'/opt/homebrew/bin/cc-moss' attach 'it'\''s "登录页"'"#)
        #expect(LaunchPlan.attachCommand(cli: cli, handle: "-x") == "'/opt/homebrew/bin/cc-moss' attach -- '-x'")
        #expect(LaunchPlan.restoreCommand(cli: cli, handle: "job-a") == "'/opt/homebrew/bin/cc-moss' restore 'job-a'")
    }

    @Test func terminalScript() throws {
        #expect(try plan(.terminal) == .appleScript("""
        tell application id "com.apple.Terminal"
        \tdo script \(LaunchPlan.appleScriptString(cmd))
        \tactivate
        end tell
        """))
    }

    @Test func ghosttyScript() throws {
        #expect(try plan(.ghostty, version: "1.3.1") == .appleScript("""
        tell application id "com.mitchellh.ghostty"
        \tactivate
        \tset cfg to new surface configuration
        \tset command of cfg to \(LaunchPlan.appleScriptString(viaShell))
        \tnew window with configuration cfg
        end tell
        """))
    }

    @Test(arguments: ["1.2.3", "1.0", nil] as [String?])
    func ghosttyBelow13FallsBack(_ version: String?) throws {
        #expect(try plan(.ghostty, version: version, app: "/Applications/Ghostty.app") == .process(
            executable: "/usr/bin/open",
            arguments: ["-na", "/Applications/Ghostty.app", "--args", "-e", "/bin/zsh", "-lc", cmd]
        ))
    }

    @Test func itermScript() throws {
        #expect(try plan(.iterm2) == .appleScript("""
        tell application id "com.googlecode.iterm2"
        \tactivate
        \tcreate window with default profile command \(LaunchPlan.appleScriptString(viaShell))
        end tell
        """))
    }

    @Test func weztermProcess() throws {
        // Running already: the new window goes into that WezTerm, not a second one.
        #expect(try plan(.wezterm, app: "/Applications/WezTerm.app") == .process(
            executable: "/Applications/WezTerm.app/Contents/MacOS/wezterm",
            arguments: ["cli", "spawn", "--new-window", "--", "/bin/zsh", "-lc", cmd]
        ))
    }

    @Test func kittyProcess() throws {
        #expect(try plan(.kitty, app: "/Applications/kitty.app") == .process(
            executable: "/usr/bin/open",
            arguments: ["-na", "/Applications/kitty.app", "--args", "/bin/zsh", "-lc", cmd]
        ))
    }

    @Test func alacrittyProcess() throws {
        #expect(try plan(.alacritty, app: "/Applications/Alacritty.app") == .process(
            executable: "/usr/bin/open",
            arguments: ["-na", "/Applications/Alacritty.app", "--args", "-e", "/bin/zsh", "-lc", cmd]
        ))
    }

    /// Through both layers: the string AppleScript would hand the terminal,
    /// run by a shell, starts `/bin/echo attach <name>` and nothing else.
    @Test(arguments: hostileNames)
    func bothLayersRoundTrip(_ hostile: String) throws {
        let plan = try LaunchPlan.make(
            for: InstalledTerminal(terminal: .ghostty, appPath: "/G.app", version: "1.3.1"),
            cli: "/bin/echo", handle: hostile, shell: "/bin/sh"
        )
        guard case .appleScript(let script) = plan,
              let line = script.split(separator: "\n").first(where: { $0.contains("set command of cfg to ") }) else {
            Issue.record("no command line in \(plan)"); return
        }
        let literal = String(line.split(separator: "set command of cfg to ", maxSplits: 1)[1])
        let handed = try #require(try evalAppleScript("return " + literal))
        let sep = hostile.hasPrefix("-") ? "-- " : ""
        #expect(try sh(handed) == "attach \(sep)\(hostile)\n")
    }

    /// The generated scripts parse against the real dictionaries where the
    /// terminal is installed (compiling sends no events and opens nothing).
    @Test(arguments: [Terminal.terminal, .ghostty, .iterm2])
    func scriptsCompile(_ t: Terminal) throws {
        let real = TerminalCatalog(appPath: { id in
            ["/System/Applications/Utilities/Terminal.app", "/Applications/Ghostty.app", "/Applications/iTerm.app"]
                .first { Bundle(path: $0)?.bundleIdentifier == id }
        })
        guard let installed = real.find(t) else { return }
        guard case .appleScript(let source) = try LaunchPlan.make(
            for: installed, cli: cli, handle: hostileNames.joined(separator: " "), shell: "/bin/zsh"
        ) else { return }
        appleScriptLock.lock()
        defer { appleScriptLock.unlock() }
        var error: NSDictionary?
        let ok = NSAppleScript(source: source)?.compileAndReturnError(&error) ?? false
        #expect(ok, "\(error ?? [:])")
    }

    @Test func runsAnUpdateCommandInTheTerminal() throws {
        let cmd = LaunchPlan.cliCommand(cli: "/opt/homebrew/bin/cc-moss", ["update"])
        #expect(cmd == "'/opt/homebrew/bin/cc-moss' 'update'")
        let t = InstalledTerminal(terminal: .terminal, appPath: "/System/Applications/Utilities/Terminal.app", version: nil)
        guard case .appleScript(let script) = try LaunchPlan.make(for: t, command: cmd, shell: "/bin/zsh") else {
            Issue.record("expected AppleScript"); return
        }
        #expect(script.contains("do script \"'/opt/homebrew/bin/cc-moss' 'update'\""))
    }
}

/// New tab or new window, and the first launch of a terminal that isn't running yet.
@Suite struct LaunchStyleTests {
    let cli = "/opt/homebrew/bin/cc-moss"
    let name = "job"
    var cmd: String { LaunchPlan.attachCommand(cli: cli, handle: name) }
    var viaShell: String { ["/bin/zsh", "-lc", cmd].map(LaunchPlan.quoteAlways).joined(separator: " ") }

    func plan(_ t: Terminal, _ context: LaunchContext, version: String? = nil, app: String = "/Applications/X.app") throws -> LaunchPlan {
        try LaunchPlan.make(for: InstalledTerminal(terminal: t, appPath: app, version: version),
                            cli: cli, handle: name, shell: "/bin/zsh", context: context)
    }

    @Test func tabIsTheDefault() {
        #expect(OpenStyle.default == .tab)
        #expect(OpenStyle(rawValue: "window") == .window)
    }

    @Test func whichTerminalsCanOpenTabs() {
        #expect(Terminal.terminal.supportsTabs(version: nil))
        #expect(Terminal.iterm2.supportsTabs(version: nil))
        #expect(Terminal.wezterm.supportsTabs(version: nil))
        #expect(Terminal.ghostty.supportsTabs(version: "1.3.1"))
        #expect(!Terminal.ghostty.supportsTabs(version: "1.2.3"))
        #expect(!Terminal.kitty.supportsTabs(version: nil))
        #expect(!Terminal.alacritty.supportsTabs(version: nil))
    }

    // MARK: Terminal

    @Test func terminalNotRunningUsesItsFirstWindow() throws {
        // Starting Terminal opens a window of its own; running the command in it
        // (rather than in another new one) is what keeps it to one window.
        let script = """
        tell application id "com.apple.Terminal"
        \tactivate
        \trepeat 50 times
        \t\tif (count of windows) > 0 then exit repeat
        \t\tdelay 0.1
        \tend repeat
        \tif (count of windows) > 0 then
        \t\tdo script \(LaunchPlan.appleScriptString(cmd)) in front window
        \telse
        \t\tdo script \(LaunchPlan.appleScriptString(cmd))
        \tend if
        end tell
        """
        #expect(try plan(.terminal, LaunchContext(style: .tab, running: false)) == .appleScript(script))
        #expect(try plan(.terminal, LaunchContext(style: .window, running: false)) == .appleScript(script))
    }

    @Test func terminalTabPressesCommandT() throws {
        #expect(try plan(.terminal, LaunchContext(style: .tab, running: true, canSendKeys: true)) == .appleScript("""
        tell application id "com.apple.Terminal"
        \tactivate
        \tif (count of windows) > 0 then
        \t\ttell application "System Events" to keystroke "t" using command down
        \t\tdelay 0.3
        \t\tdo script \(LaunchPlan.appleScriptString(cmd)) in front window
        \telse
        \t\tdo script \(LaunchPlan.appleScriptString(cmd))
        \tend if
        end tell
        """))
    }

    @Test func terminalTabWithoutKeysIsAWindow() throws {
        // Terminal has no "new tab" command; without permission to press ⌘T, a window it is.
        #expect(try plan(.terminal, LaunchContext(style: .tab, running: true, canSendKeys: false))
            == plan(.terminal, LaunchContext(style: .window, running: true)))
    }

    // MARK: Ghostty

    @Test func ghosttyTab() throws {
        #expect(try plan(.ghostty, LaunchContext(style: .tab, running: true), version: "1.3.1") == .appleScript("""
        tell application id "com.mitchellh.ghostty"
        \tactivate
        \tset cfg to new surface configuration
        \tset command of cfg to \(LaunchPlan.appleScriptString(viaShell))
        \tif (count of windows) > 0 then
        \t\tnew tab in front window with configuration cfg
        \telse
        \t\tnew window with configuration cfg
        \tend if
        end tell
        """))
    }

    @Test func ghosttyNotRunningStartsWithTheCommand() throws {
        #expect(try plan(.ghostty, LaunchContext(style: .tab, running: false), version: "1.3.1", app: "/Applications/Ghostty.app") == .process(
            executable: "/usr/bin/open",
            arguments: ["-a", "/Applications/Ghostty.app", "--args", "-e", "/bin/zsh", "-lc", cmd]
        ))
    }

    // MARK: iTerm2

    @Test func itermTab() throws {
        #expect(try plan(.iterm2, LaunchContext(style: .tab, running: true)) == .appleScript("""
        tell application id "com.googlecode.iterm2"
        \tactivate
        \tif (count of windows) > 0 then
        \t\ttell current window to create tab with default profile command \(LaunchPlan.appleScriptString(viaShell))
        \telse
        \t\tcreate window with default profile command \(LaunchPlan.appleScriptString(viaShell))
        \tend if
        end tell
        """))
    }

    @Test func itermNotRunningUsesItsFirstWindow() throws {
        #expect(try plan(.iterm2, LaunchContext(style: .window, running: false)) == .appleScript("""
        tell application id "com.googlecode.iterm2"
        \tactivate
        \trepeat 50 times
        \t\tif (count of windows) > 0 then exit repeat
        \t\tdelay 0.1
        \tend repeat
        \tif (count of windows) > 0 then
        \t\ttell current session of current window to write text \(LaunchPlan.appleScriptString(cmd))
        \telse
        \t\tcreate window with default profile command \(LaunchPlan.appleScriptString(viaShell))
        \tend if
        end tell
        """))
    }

    // MARK: WezTerm

    @Test func weztermSpawnsIntoTheRunningOne() throws {
        let exe = "/Applications/WezTerm.app/Contents/MacOS/wezterm"
        #expect(try plan(.wezterm, LaunchContext(style: .tab, running: true), app: "/Applications/WezTerm.app")
            == .process(executable: exe, arguments: ["cli", "spawn", "--", "/bin/zsh", "-lc", cmd]))
        #expect(try plan(.wezterm, LaunchContext(style: .window, running: true), app: "/Applications/WezTerm.app")
            == .process(executable: exe, arguments: ["cli", "spawn", "--new-window", "--", "/bin/zsh", "-lc", cmd]))
        #expect(try plan(.wezterm, LaunchContext(style: .tab, running: false), app: "/Applications/WezTerm.app")
            == .process(executable: exe, arguments: ["start", "--", "/bin/zsh", "-lc", cmd]))
    }

    // MARK: no tabs

    @Test func kittyAndAlacrittyAlwaysOpenAWindow() throws {
        for t in [Terminal.kitty, .alacritty] {
            #expect(try plan(t, LaunchContext(style: .tab, running: true)) == plan(t, LaunchContext(style: .window, running: true)))
        }
    }
}
