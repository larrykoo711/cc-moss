import Foundation

/// How to open a terminal tab or window running `cc-moss attach <handle>`:
/// either an AppleScript for the terminal, or a program to start.
///
/// Session names come from users, so they are treated as hostile twice:
/// shell-quoted inside the command, and escaped again when that command is
/// written into an AppleScript string. Names with control characters are
/// refused outright: a terminal that types the command into its shell
/// (Terminal's `do script`) would let ^C, ^U or a newline act as keystrokes.
/// What the plan depends on besides the terminal: the style the user picked,
/// whether the terminal is running already (a terminal that starts opens a
/// window of its own, which the command should use rather than add a second),
/// and whether cc-moss may press keys (Terminal has no "new tab" command, so a
/// tab there is ⌘T, which needs Accessibility).
public struct LaunchContext: Equatable, Sendable {
    public var style: OpenStyle
    public var running: Bool
    public var canSendKeys: Bool

    public init(style: OpenStyle = .window, running: Bool = true, canSendKeys: Bool = false) {
        self.style = style
        self.running = running
        self.canSendKeys = canSendKeys
    }
}

public enum LaunchPlan: Equatable, Sendable {
    case appleScript(String)
    case process(executable: String, arguments: [String])

    public enum Failure: Error, Equatable {
        /// The handle holds a control character; copy the command instead.
        case unsafeName
    }

    /// The command that takes you into a session, for any terminal or for
    /// pasting: `'<cc-moss>' attach '<handle>'`.
    public static func attachCommand(cli: String, handle: String) -> String {
        command(cli: cli, verb: "attach", handle: handle)
    }

    /// The command that brings an interrupted session back.
    public static func restoreCommand(cli: String, handle: String) -> String {
        command(cli: cli, verb: "restore", handle: handle)
    }

    private static func command(cli: String, verb: String, handle: String) -> String {
        // A name starting with "-" would read as an option; `--` ends options.
        let separator = handle.hasPrefix("-") ? " --" : ""
        return quoteAlways(cli) + " " + verb + separator + " " + quoteAlways(handle)
    }

    /// Single-quote a word for a POSIX shell, even when it is plainly safe,
    /// so the command always has the same shape.
    static func quoteAlways(_ s: String) -> String {
        "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    /// The plan for `terminal`. `shell` is the user's login shell, which runs
    /// the command where the terminal would otherwise run it directly, so
    /// the PATH (and `tmux`) is the one the user knows.
    public static func make(
        for terminal: InstalledTerminal, cli: String, handle: String, shell: String,
        context: LaunchContext = LaunchContext()
    ) throws -> LaunchPlan {
        guard !hasControlCharacters(handle), !hasControlCharacters(cli) else {
            throw Failure.unsafeName
        }
        return try make(for: terminal, command: attachCommand(cli: cli, handle: handle), shell: shell, context: context)
    }

    /// `cc-moss update` (or another cc-moss command) in a new terminal
    /// window, so the user sees what it does.
    public static func cliCommand(cli: String, _ words: [String]) -> String {
        ([quoteAlways(cli)] + words.map(quoteAlways)).joined(separator: " ")
    }

    /// The plan for running an already quoted shell `command` in `terminal`.
    public static func make(
        for terminal: InstalledTerminal, command cmd: String, shell: String,
        context: LaunchContext = LaunchContext()
    ) throws -> LaunchPlan {
        guard !hasControlCharacters(cmd), !hasControlCharacters(shell) else {
            throw Failure.unsafeName
        }
        let viaShell = [shell, "-lc", cmd]
        let viaShellLine = viaShell.map(quoteAlways).joined(separator: " ")
        let app = terminal.appPath
        let id = terminal.terminal.bundleID
        let tab = context.style == .tab
        /// Wait (up to 5 s) for the window a terminal opens when it starts.
        let awaitFirstWindow = """
        \trepeat 50 times
        \t\tif (count of windows) > 0 then exit repeat
        \t\tdelay 0.1
        \tend repeat
        """

        switch terminal.terminal {
        case .terminal where !context.running:
            // Starting Terminal opens a window; the command runs in it, so the
            // first open is one window, not two.
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \(awaitFirstWindow)
            \tif (count of windows) > 0 then
            \t\tdo script \(appleScriptString(cmd)) in front window
            \telse
            \t\tdo script \(appleScriptString(cmd))
            \tend if
            end tell
            """)
        case .terminal where tab && context.canSendKeys:
            // No "new tab" command in Terminal's dictionary: ⌘T, then run the
            // command in the tab it made.
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \tif (count of windows) > 0 then
            \t\ttell application "System Events" to keystroke "t" using command down
            \t\tdelay 0.3
            \t\tdo script \(appleScriptString(cmd)) in front window
            \telse
            \t\tdo script \(appleScriptString(cmd))
            \tend if
            end tell
            """)
        case .terminal:
            // `do script` types the line into the user's own shell.
            return .appleScript("""
            tell application id "\(id)"
            \tdo script \(appleScriptString(cmd))
            \tactivate
            end tell
            """)
        case .ghostty where Version.atLeast(terminal.version, [1, 3]) && !context.running:
            // Started with the command, Ghostty's first window is the session.
            return .process(executable: "/usr/bin/open", arguments: ["-a", app, "--args", "-e"] + viaShell)
        case .ghostty where Version.atLeast(terminal.version, [1, 3]) && tab:
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \tset cfg to new surface configuration
            \tset command of cfg to \(appleScriptString(viaShellLine))
            \tif (count of windows) > 0 then
            \t\tnew tab in front window with configuration cfg
            \telse
            \t\tnew window with configuration cfg
            \tend if
            end tell
            """)
        case .ghostty where Version.atLeast(terminal.version, [1, 3]):
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \tset cfg to new surface configuration
            \tset command of cfg to \(appleScriptString(viaShellLine))
            \tnew window with configuration cfg
            end tell
            """)
        case .ghostty:
            // No scripting before 1.3: start a new instance with `-e`.
            return .process(executable: "/usr/bin/open", arguments: ["-na", app, "--args", "-e"] + viaShell)
        case .iterm2 where !context.running:
            // iTerm opens a window as it starts; the command goes into it.
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \(awaitFirstWindow)
            \tif (count of windows) > 0 then
            \t\ttell current session of current window to write text \(appleScriptString(cmd))
            \telse
            \t\tcreate window with default profile command \(appleScriptString(viaShellLine))
            \tend if
            end tell
            """)
        case .iterm2 where tab:
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \tif (count of windows) > 0 then
            \t\ttell current window to create tab with default profile command \(appleScriptString(viaShellLine))
            \telse
            \t\tcreate window with default profile command \(appleScriptString(viaShellLine))
            \tend if
            end tell
            """)
        case .iterm2:
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \tcreate window with default profile command \(appleScriptString(viaShellLine))
            end tell
            """)
        case .wezterm where context.running:
            // `cli spawn` goes into the running WezTerm: a tab, or a window.
            let spawn = tab ? ["cli", "spawn", "--"] : ["cli", "spawn", "--new-window", "--"]
            return .process(executable: app + "/Contents/MacOS/wezterm", arguments: spawn + viaShell)
        case .wezterm:
            return .process(executable: app + "/Contents/MacOS/wezterm", arguments: ["start", "--"] + viaShell)
        case .kitty:
            return .process(executable: "/usr/bin/open", arguments: ["-na", app, "--args"] + viaShell)
        case .alacritty:
            return .process(executable: "/usr/bin/open", arguments: ["-na", app, "--args", "-e"] + viaShell)
        }
    }

    /// An AppleScript string literal: backslashes and double quotes escaped.
    static func appleScriptString(_ s: String) -> String {
        "\"" + s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
    }

    static func hasControlCharacters(_ s: String) -> Bool {
        s.unicodeScalars.contains { $0.properties.generalCategory == .control }
    }
}
