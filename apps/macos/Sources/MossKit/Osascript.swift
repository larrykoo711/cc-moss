import Foundation

/// Running a terminal's AppleScript through `/usr/bin/osascript` (a process
/// of its own, so nothing in the app waits or runs AppleScript off the main
/// thread), and reading how it went from its exit status and error line.
/// The Automation prompt still names the app: macOS asks for the process
/// responsible for osascript.
public enum Osascript {
    public static let executable = "/usr/bin/osascript"
    /// The script comes on standard input.
    public static let arguments = ["-"]

    public enum Result: Equatable, Sendable {
        case opened
        /// Automation was refused (-1743).
        case denied
        case failed(String)
    }

    /// `osascript` writes "<range>: execution error: <message> (<number>)".
    public static func result(status: Int32, stderr: String) -> Result {
        guard status != 0 else { return .opened }
        let line = stderr.trimmingCharacters(in: .whitespacesAndNewlines)
        if line.hasSuffix("(-1743)") { return .denied }
        guard !line.isEmpty else { return .failed("AppleScript") }
        if let range = line.range(of: "execution error: ") {
            return .failed(String(line[range.upperBound...]))
        }
        return .failed(line)
    }
}
