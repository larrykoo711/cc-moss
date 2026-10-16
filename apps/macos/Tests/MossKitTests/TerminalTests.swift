import Foundation
import Testing
@testable import MossKit

/// A catalog where exactly `installed` (bundle id → version) exist.
func catalog(_ installed: [Terminal: String?]) -> TerminalCatalog {
    let byID = Dictionary(uniqueKeysWithValues: installed.map { ($0.key.bundleID, ($0.key, $0.value)) })
    return TerminalCatalog(
        appPath: { id in byID[id].map { "/Applications/\($0.0.rawValue).app" } },
        version: { path in byID.values.first { "/Applications/\($0.0.rawValue).app" == path }?.1 ?? nil }
    )
}

/// Names a user could give a session, chosen to break naive quoting.
let hostileNames: [String] = [
    "job-a",
    "my job",
    "it's",
    "say \"hi\"",
    #"back\slash"#,
    #"\""#,
    #"'\''"#,
    "登录页 修复",
    "$(touch /tmp/pwned)",
    "`id`",
    "a; rm -rf ~",
    #""; do shell script "touch /tmp/pwned"; ""#,
    #"\"; do shell script \"id\" --"#,
    "-n",
    "--help",
    "emoji 👩‍💻 zwj",
]

@Suite struct TerminalTests {
    @Test func defaultIsSystemTerminal() {
        #expect(Terminal.default == .terminal)
        // Even with every other terminal installed, nothing chosen means Terminal.
        let all = catalog(Dictionary(uniqueKeysWithValues: Terminal.allCases.map { ($0, "9.9") }))
        #expect(all.resolve(nil).installed?.terminal == .terminal)
        #expect(all.resolve(nil).fellBackFrom == nil)
    }

    @Test func detectsOnlyInstalled() {
        let c = catalog([.terminal: "2.15", .ghostty: "1.3.1"])
        #expect(c.installed().map(\.terminal) == [.terminal, .ghostty])
        #expect(c.installed()[1] == InstalledTerminal(terminal: .ghostty, appPath: "/Applications/ghostty.app", version: "1.3.1"))
        #expect(c.find(.iterm2) == nil)
    }

    @Test func ghosttyIsRecommended() {
        #expect(Terminal.allCases.filter(\.isRecommended) == [.ghostty])
        #expect(Terminal.ghostty.downloadURL?.host == "ghostty.org")
    }

    @Test func fallsBackWhenChosenIsGone() {
        let c = catalog([.terminal: "2.15"])
        let choice = c.resolve(.ghostty)
        #expect(choice.installed?.terminal == .terminal)
        #expect(choice.fellBackFrom == .ghostty)
        // A choice that is still there sticks.
        #expect(catalog([.terminal: nil, .kitty: "0.40"]).resolve(.kitty).installed?.terminal == .kitty)
    }

    @Test func bundleIDs() {
        #expect(Terminal.allCases.map(\.bundleID) == [
            "com.apple.Terminal", "com.mitchellh.ghostty", "com.googlecode.iterm2",
            "com.github.wez.wezterm", "net.kovidgoyal.kitty", "org.alacritty",
        ])
    }

    @Test(arguments: [
        ("1.3.1", true), ("1.3", true), ("1.3.0-dev+abc", true), ("2.0", true), ("1.10.0", true),
        ("1.2.3", false), ("1.2", false), ("0.9", false), ("tip", false), ("", false),
    ])
    func versionGate(_ v: String, _ ok: Bool) {
        #expect(Version.atLeast(v, [1, 3]) == ok)
    }

    @Test func escapesQuotesAndBackslashes() throws {
        // The literal AppleScript sees, escaped for a double-quoted string.
        #expect(LaunchPlan.appleScriptString(#"a "b" \c"#) == #""a \"b\" \\c""#)
        // The shell word, single-quoted with '\'' for the quote.
        #expect(LaunchPlan.attachCommand(cli: "/opt/x/cc-moss", handle: "it's") == #"'/opt/x/cc-moss' attach 'it'\''s'"#)
        // Both layers together, in Terminal's script.
        let plan = try LaunchPlan.make(
            for: InstalledTerminal(terminal: .terminal, appPath: "/T.app"),
            cli: "/opt/x/cc-moss", handle: #"say "hi" \ it's"#, shell: "/bin/zsh"
        )
        guard case .appleScript(let script) = plan else { Issue.record("not a script"); return }
        #expect(script.contains(#"do script "'/opt/x/cc-moss' attach 'say \"hi\" \\ it'\\''s'""#))
    }

    @Test(arguments: hostileNames)
    func shellRoundTrip(_ name: String) throws {
        // `/bin/echo attach <name>` must print the name exactly: the shell
        // saw one word, with nothing run along the way.
        let out = try sh(LaunchPlan.attachCommand(cli: "/bin/echo", handle: name))
        let sep = name.hasPrefix("-") ? "-- " : ""
        #expect(out == "attach \(sep)\(name)\n")
    }

    @Test(arguments: hostileNames)
    func appleScriptRoundTrip(_ name: String) throws {
        // Evaluating the escaped literal gives back the very same string.
        let literal = LaunchPlan.appleScriptString(LaunchPlan.attachCommand(cli: "/bin/echo", handle: name))
        let back = try evalAppleScript("return " + literal)
        #expect(back == LaunchPlan.attachCommand(cli: "/bin/echo", handle: name))
    }

    @Test(arguments: ["a\nb", "a\rb", "x\u{15}rm -rf ~", "tab\there", "nul\u{0}", "del\u{7f}"])
    func refusesControlCharacters(_ name: String) {
        #expect(throws: LaunchPlan.Failure.unsafeName) {
            try LaunchPlan.make(for: InstalledTerminal(terminal: .terminal, appPath: "/T.app"),
                                cli: "/c", handle: name, shell: "/bin/zsh")
        }
    }
}

/// Run a command line with /bin/sh and return what it printed.
func sh(_ line: String) throws -> String {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/bin/sh")
    p.arguments = ["-c", line]
    let out = Pipe()
    p.standardOutput = out
    try p.run()
    let data = out.fileHandleForReading.readDataToEndOfFile()
    p.waitUntilExit()
    return String(decoding: data, as: UTF8.self)
}

/// NSAppleScript is not thread-safe and Swift Testing runs cases in parallel.
let appleScriptLock = NSLock()

/// Evaluate a pure AppleScript (no `tell`, so no app is touched).
func evalAppleScript(_ source: String) throws -> String? {
    appleScriptLock.lock()
    defer { appleScriptLock.unlock() }
    var error: NSDictionary?
    let result = NSAppleScript(source: source)?.executeAndReturnError(&error)
    if let error { throw AppleScriptError(info: "\(error)") }
    return result?.stringValue
}

struct AppleScriptError: Error { let info: String }
