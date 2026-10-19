import AppKit
import MossKit
import Observation

extension TerminalCatalog {
    /// Asks LaunchServices where each terminal lives.
    static let live = TerminalCatalog(appPath: { id in
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: id)?.path
    })

    /// Only the listed terminals count as installed; for screenshots.
    static func only(_ terminals: [Terminal]) -> TerminalCatalog {
        let ids = Set(terminals.map(\.bundleID))
        return TerminalCatalog(appPath: { ids.contains($0) ? TerminalCatalog.live.appPath($0) : nil })
    }
}

/// Which terminal sessions open in: the user's pick (kept in UserDefaults),
/// what is installed, and a note when the pick has gone missing.
@MainActor @Observable
final class TerminalPrefs {
    @ObservationIgnored let catalog: TerminalCatalog
    @ObservationIgnored private let defaults: UserDefaults
    private static let key = "terminal"
    private static let styleKey = "terminalOpenStyle"

    private(set) var installed: [InstalledTerminal] = []
    /// nil until the user picks one: then the system terminal.
    private(set) var chosen: Terminal?
    /// The pick that went missing; the settings mention it once.
    var notice: Terminal?
    /// New tab (the default) or new window.
    var openStyle: OpenStyle {
        didSet { defaults.set(openStyle.rawValue, forKey: Self.styleKey) }
    }

    init(catalog: TerminalCatalog = .live, defaults: UserDefaults = .standard) {
        self.catalog = catalog
        self.defaults = defaults
        chosen = defaults.string(forKey: Self.key).flatMap(Terminal.init(rawValue:))
        openStyle = defaults.string(forKey: Self.styleKey).flatMap(OpenStyle.init(rawValue:)) ?? .default
        rescan()
    }

    /// Look again at what is installed (cheap; done when a window opens).
    func rescan() {
        installed = catalog.installed()
    }

    /// What sessions open in now. A pick that is gone falls back to the
    /// system terminal, once, with a note for the settings.
    var current: InstalledTerminal? {
        let wanted = chosen ?? .default
        return installed.first { $0.terminal == wanted } ?? installed.first { $0.terminal == .default }
    }

    /// Checks the pick is still installed; forgets it (and notes it) if not.
    func settle() {
        rescan()
        let choice = catalog.resolve(chosen)
        if let gone = choice.fellBackFrom {
            notice = gone
            choose(nil)
        }
    }

    func choose(_ t: Terminal?) {
        chosen = t
        if let t {
            defaults.set(t.rawValue, forKey: Self.key)
            notice = nil
        } else {
            defaults.removeObject(forKey: Self.key)
        }
    }

    var ghosttyMissing: Bool { !installed.contains { $0.terminal == .ghostty } }

    static func icon(_ t: InstalledTerminal, size: CGFloat) -> NSImage {
        let image = NSWorkspace.shared.icon(forFile: t.appPath)
        image.size = NSSize(width: size, height: size)
        return image
    }
}

/// Why a session did not open, with the command to paste instead.
struct OpenProblem: Equatable {
    enum Kind: Equatable {
        /// macOS said cc-moss may not control the terminal (Automation).
        case denied
        case failed(String)
        /// The name has characters a terminal could take for keystrokes.
        case unsafeName
        /// No supported terminal found at all.
        case noTerminal
    }

    let kind: Kind
    let terminalName: String
    let command: String
}

/// Carries out launch plans.
@MainActor
enum Opener {
    enum Outcome: Equatable, Sendable {
        case opened
        case denied
        case failed(String)
    }

    /// The Automation pane of Privacy & Security.
    static let automationSettings = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation")!

    /// The user's login shell: `$SHELL`, else the account's, else zsh.
    static var loginShell: String {
        if let s = ProcessInfo.processInfo.environment["SHELL"], s.hasPrefix("/") { return s }
        if let pw = getpwuid(getuid()), let sh = pw.pointee.pw_shell {
            let s = String(cString: sh)
            if s.hasPrefix("/") { return s }
        }
        return "/bin/zsh"
    }

    /// Run an already quoted cc-moss `command` (e.g. `cc-moss update`) in a
    /// new window of `terminal`, so the user watches it happen.
    /// What the plan needs to know about this moment: the user's style, whether
    /// the terminal is up, and whether cc-moss may press ⌘T (Accessibility).
    static func context(for terminal: InstalledTerminal, style: OpenStyle) -> LaunchContext {
        let running = !NSRunningApplication.runningApplications(withBundleIdentifier: terminal.terminal.bundleID).isEmpty
        return LaunchContext(style: style, running: running, canSendKeys: AXIsProcessTrusted())
    }

    static func run(command: String, in terminal: InstalledTerminal?, style: OpenStyle = .window) async -> OpenProblem? {
        guard let terminal else {
            return OpenProblem(kind: .noTerminal, terminalName: L.t("a terminal", "终端"), command: command)
        }
        let name = terminal.terminal.displayName
        let plan: LaunchPlan
        do {
            plan = try LaunchPlan.make(for: terminal, command: command, shell: loginShell,
                                       context: context(for: terminal, style: style))
        } catch {
            return OpenProblem(kind: .unsafeName, terminalName: name, command: command)
        }
        switch await run(plan, terminal: terminal) {
        case .opened: return nil
        case .denied: return OpenProblem(kind: .denied, terminalName: name, command: command)
        case .failed(let why): return OpenProblem(kind: .failed(why), terminalName: name, command: command)
        }
    }

    /// Open `handle` in `terminal`. The plan is made here; carrying it out
    /// (AppleScript, `open`) happens off the main thread, so the app never
    /// waits on it, including the first time, when macOS asks for Automation
    /// permission.
    static func open(handle: String, cli: String, in terminal: InstalledTerminal?, style: OpenStyle = .default) async -> OpenProblem? {
        let command = LaunchPlan.attachCommand(cli: cli, handle: handle)
        guard let terminal else {
            return OpenProblem(kind: .noTerminal, terminalName: L.t("a terminal", "终端"), command: command)
        }
        let name = terminal.terminal.displayName
        let plan: LaunchPlan
        do {
            plan = try LaunchPlan.make(for: terminal, cli: cli, handle: handle, shell: loginShell,
                                       context: context(for: terminal, style: style))
        } catch {
            return OpenProblem(kind: .unsafeName, terminalName: name, command: command)
        }
        switch await run(plan, terminal: terminal) {
        case .opened: return nil
        case .denied: return OpenProblem(kind: .denied, terminalName: name, command: command)
        case .failed(let why): return OpenProblem(kind: .failed(why), terminalName: name, command: command)
        }
    }

    static func run(_ plan: LaunchPlan, terminal: InstalledTerminal) async -> Outcome {
        let outcome = await execute(plan)
        // A tab spawned from the command line (WezTerm) doesn't bring its app
        // forward on its own; scripted terminals already said `activate`.
        if outcome == .opened, case .process = plan {
            NSRunningApplication.runningApplications(withBundleIdentifier: terminal.terminal.bundleID).first?.activate()
        }
        return outcome
    }

    /// One plan at a time, on a queue of its own: NSAppleScript is used from
    /// one thread at a time, and the main thread is never the one waiting.
    private nonisolated static let queue = DispatchQueue(label: "cc-moss.opener", qos: .userInitiated)

    nonisolated static func execute(_ plan: LaunchPlan) async -> Outcome {
        await withCheckedContinuation { continuation in
            queue.async { continuation.resume(returning: runBlocking(plan)) }
        }
    }

    /// Carries the plan out and waits for it: never call on the main thread.
    nonisolated static func runBlocking(_ plan: LaunchPlan) -> Outcome {
        switch plan {
        case .appleScript(let source):
            guard let script = NSAppleScript(source: source) else { return .failed("AppleScript") }
            var error: NSDictionary?
            script.executeAndReturnError(&error)
            guard let error else { return .opened }
            let number = error[NSAppleScript.errorNumber] as? Int ?? 0
            // -1743: the user (or an MDM profile) said no to Automation.
            if number == -1743 { return .denied }
            let message = error[NSAppleScript.errorMessage] as? String ?? "AppleScript"
            return .failed("\(message) (\(number))")
        case .process(let executable, let arguments):
            let process = Process()
            process.executableURL = URL(fileURLWithPath: executable)
            process.arguments = arguments
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = FileHandle.nullDevice
            let err = Pipe()
            process.standardError = err
            do {
                try process.run()
            } catch {
                return .failed(error.localizedDescription)
            }
            // `open` hands over and exits at once; a terminal binary started
            // directly (WezTerm) keeps running, so only `open` is waited on.
            guard executable == "/usr/bin/open" else { return .opened }
            process.waitUntilExit()
            if process.terminationStatus == 0 { return .opened }
            let why = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            return .failed(why.trimmingCharacters(in: .whitespacesAndNewlines))
        }
    }
}
