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

    /// Is there a newer cc-moss? From `cc-moss update --check --json`.
    public func updateInfo() async throws -> UpdateInfo {
        let out = try await plain(["update", "--check", "--json"], timeout: 20)
        do {
            return try UpdateInfo.decode(Data(out.utf8))
        } catch {
            throw Failure.unreadable("\(error)")
        }
    }

    /// What `cc-moss update --check` says, in its own words.
    public func updateCheck() async throws -> String {
        try await plain(["update", "--check"], timeout: 20)
    }

    /// A setting the CLI owns (`cc-moss config get <key> --json`), e.g. `ui.language`.
    public func config(_ key: String) async throws -> String {
        try ConfigValue.decode(Data(try await plain(["config", "get", key, "--json"]).utf8))
    }

    /// Change it through the CLI, which keeps the file's comments and layout.
    public func setConfig(_ key: String, _ value: String) async throws {
        _ = try ConfigValue.decode(Data(try await plain(["config", "set", key, value, "--json"]).utf8))
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

/// The answer of `cc-moss update --check --json` (schema 1).
public struct UpdateInfo: Decodable, Equatable, Sendable {
    public var current: String
    public var latest: String
    public var available: Bool
    public var notes: String?

    public static func decode(_ data: Data) throws -> UpdateInfo {
        struct Head: Decodable { var schema: Int }
        let head = try JSONDecoder().decode(Head.self, from: data)
        guard head.schema <= 1 else { throw SnapshotError.newerSchema(head.schema) }
        return try JSONDecoder().decode(UpdateInfo.self, from: data)
    }
}

/// The answer of `cc-moss config get|set --json` (schema 1).
enum ConfigValue {
    private struct Body: Decodable {
        var schema: Int
        var value: String
    }

    static func decode(_ data: Data) throws -> String {
        do {
            return try JSONDecoder().decode(Body.self, from: data).value
        } catch {
            throw MossClient.Failure.unreadable("\(error)")
        }
    }
}
