import Foundation

/// The app's only way to the session registry: the cc-moss CLI.
public struct MossClient: Sendable {
    public var executable: String
    public var runner: CommandRunner
    public var timeout: Double

    public init(executable: String, runner: CommandRunner = ProcessRunner(), timeout: Double = 10) {
        self.executable = executable
        self.runner = runner
        self.timeout = timeout
    }

    public enum Failure: Error, Equatable {
        /// cc-moss ran and said no; `message` is the first line it printed.
        case command(String)
        case newerSchema(Int)
        case unreadable(String)
        case timedOut
    }

    public func snapshot() async throws -> Snapshot {
        let result: CommandResult
        do {
            result = try await runner.run(executable, ["ls", "--json"], timeout: timeout)
        } catch RunError.timedOut {
            throw Failure.timedOut
        } catch {
            throw Failure.command("\(error)")
        }
        guard result.status == 0 else {
            throw Failure.command(firstLine(result.stderr) ?? L.t("cc-moss exited with \(result.status)", "cc-moss 退出码 \(result.status)"))
        }
        do {
            return try Snapshot.decode(result.stdout)
        } catch SnapshotError.newerSchema(let n) {
            throw Failure.newerSchema(n)
        } catch {
            throw Failure.unreadable("\(error)")
        }
    }
}

extension MossClient {
    /// `0.1.1`, from `cc-moss --version`.
    public func version() async throws -> String {
        let out = try await plain(["--version"])
        return out.split(separator: " ").last.map(String.init) ?? out
    }

    /// What `cc-moss update --check` says, in its own words.
    public func updateCheck() async throws -> String {
        try await plain(["update", "--check"], timeout: 20)
    }

    /// Run a command and return its output, or its error line as a failure.
    func plain(_ args: [String], timeout: Double? = nil) async throws -> String {
        let r: CommandResult
        do {
            r = try await runner.run(executable, args, timeout: timeout ?? self.timeout)
        } catch RunError.timedOut {
            throw Failure.timedOut
        }
        let out = String(decoding: r.stdout, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard r.status == 0 else {
            throw Failure.command(firstLine(r.stderr) ?? firstLine(out) ?? "exit \(r.status)")
        }
        return out
    }
}

/// The first non-empty line, without the `cc-moss:` / `error:` noise.
func firstLine(_ text: String) -> String? {
    text.split(whereSeparator: \.isNewline)
        .map { $0.trimmingCharacters(in: .whitespaces) }
        .first { !$0.isEmpty }
}
