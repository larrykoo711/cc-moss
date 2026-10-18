import Foundation

public struct CommandResult: Equatable, Sendable {
    public var status: Int32
    public var stdout: Data
    public var stderr: String

    public init(status: Int32, stdout: Data, stderr: String) {
        self.status = status
        self.stdout = stdout
        self.stderr = stderr
    }
}

public enum RunError: Error, Equatable {
    case timedOut(seconds: Double)
    case launchFailed(String)
}

/// Runs a command. The app talks to cc-moss only through this, so tests can
/// swap in a fake.
public protocol CommandRunner: Sendable {
    func run(_ executable: String, _ arguments: [String], timeout: Double) async throws -> CommandResult
}

/// The PATH a command gets: the executable's own folder, then Homebrew's,
/// then the system's. cc-moss runs `tmux` by name, which a bare app PATH
/// would not find.
public func augmentedPath(for executable: String, current: String?) -> String {
    var dirs = [(executable as NSString).deletingLastPathComponent,
                "/opt/homebrew/bin", "/usr/local/bin"]
    dirs += (current ?? "").split(separator: ":").map(String.init)
    dirs += ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
    var seen = Set<String>()
    return dirs.filter { !$0.isEmpty && seen.insert($0).inserted }.joined(separator: ":")
}

public struct ProcessRunner: CommandRunner {
    public var environment: [String: String]

    public init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        self.environment = environment
    }

    public func run(_ executable: String, _ arguments: [String], timeout: Double) async throws -> CommandResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        var env = environment
        env["PATH"] = augmentedPath(for: executable, current: env["PATH"])
        process.environment = env
        let out = Pipe(), err = Pipe()
        process.standardOutput = out
        process.standardError = err
        process.standardInput = FileHandle.nullDevice

        // Read both pipes while the process runs: a full pipe would block it.
        let outData = Task.detached { out.fileHandleForReading.readDataToEndOfFile() }
        let errData = Task.detached { err.fileHandleForReading.readDataToEndOfFile() }
        let exit = Exit()
        process.terminationHandler = { _ in exit.fire() }
        do {
            try process.run()
        } catch {
            throw RunError.launchFailed("\(error.localizedDescription)")
        }
        // Too slow, or nobody wants the answer any more: stop the command, then
        // still wait for it to end. Asking a running process for its exit
        // status raises an exception that takes the whole app down.
        let child = Child(process: process)
        let timer = DispatchWorkItem { exit.timeOut(); child.stop() }
        DispatchQueue.global().asyncAfter(deadline: .now() + timeout, execute: timer)
        await withTaskCancellationHandler { await exit.wait() } onCancel: { child.stop() }
        timer.cancel()
        let stdout = await outData.value
        let stderr = String(decoding: await errData.value, as: UTF8.self)
        if exit.timedOut { throw RunError.timedOut(seconds: timeout) }
        try Task.checkCancellation()
        return CommandResult(status: process.terminationStatus, stdout: stdout, stderr: stderr)
    }
}

/// The command's end, told once from whichever thread sees it.
private final class Exit: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false
    private var timedOutFlag = false
    private var waiter: CheckedContinuation<Void, Never>?

    var timedOut: Bool { lock.withLock { timedOutFlag } }

    func timeOut() { lock.withLock { if !done { timedOutFlag = true } } }

    func fire() {
        let w: CheckedContinuation<Void, Never>? = lock.withLock {
            done = true
            defer { waiter = nil }
            return waiter
        }
        w?.resume()
    }

    func wait() async {
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            let ended = lock.withLock {
                if !done { waiter = c }
                return done
            }
            if ended { c.resume() }
        }
    }
}

/// The running process, handed to the timer and the cancel handler.
private struct Child: @unchecked Sendable {
    let process: Process
    func stop() { if process.isRunning { process.terminate() } }
}

/// Answers every command with the same output. Used for demo screenshots
/// and tests, never to talk to a real cc-moss.
public struct CannedRunner: CommandRunner {
    public var result: CommandResult

    public init(stdout: Data, status: Int32 = 0, stderr: String = "") {
        result = CommandResult(status: status, stdout: stdout, stderr: stderr)
    }

    public func run(_ executable: String, _ arguments: [String], timeout: Double) async throws -> CommandResult {
        result
    }
}
