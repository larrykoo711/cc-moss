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
        #expect(try plan(.wezterm, app: "/Applications/WezTerm.app") == .process(
            executable: "/Applications/WezTerm.app/Contents/MacOS/wezterm",
            arguments: ["start", "--", "/bin/zsh", "-lc", cmd]
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
}
