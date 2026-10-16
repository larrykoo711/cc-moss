import Foundation

/// How to open a new terminal window running `cc-moss attach <handle>`:
/// either an AppleScript for the terminal, or a program to start.
///
/// Session names come from users, so they are treated as hostile twice:
/// shell-quoted inside the command, and escaped again when that command is
/// written into an AppleScript string. Names with control characters are
/// refused outright: a terminal that types the command into its shell
/// (Terminal's `do script`) would let ^C, ^U or a newline act as keystrokes.
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
        for terminal: InstalledTerminal, cli: String, handle: String, shell: String
    ) throws -> LaunchPlan {
        guard !hasControlCharacters(handle), !hasControlCharacters(cli), !hasControlCharacters(shell) else {
            throw Failure.unsafeName
        }
        let cmd = attachCommand(cli: cli, handle: handle)
        let viaShell = [shell, "-lc", cmd]
        let viaShellLine = viaShell.map(quoteAlways).joined(separator: " ")
        let app = terminal.appPath
        let id = terminal.terminal.bundleID

        switch terminal.terminal {
        case .terminal:
            // `do script` types the line into the user's own shell.
            return .appleScript("""
            tell application id "\(id)"
            \tdo script \(appleScriptString(cmd))
            \tactivate
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
        case .iterm2:
            return .appleScript("""
            tell application id "\(id)"
            \tactivate
            \tcreate window with default profile command \(appleScriptString(viaShellLine))
            end tell
            """)
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
