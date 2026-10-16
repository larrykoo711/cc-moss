import Foundation

/// One native alias, as `cc-moss alias ls --json` describes it.
public struct Alias: Decodable, Equatable, Identifiable, Sendable {
    public var name: String
    /// What it runs after `cc-moss`, e.g. `new --detach`.
    public var command: String
    public var status: Status
    /// What is wrong, in cc-moss's words (already in the app's language).
    public var detail: String?
    /// How to fix it.
    public var fix: String?

    public var id: String { name }

    public init(name: String, command: String, status: Status = .ok, detail: String? = nil, fix: String? = nil) {
        self.name = name
        self.command = command
        self.status = status
        self.detail = detail
        self.fix = fix
    }

    /// The most serious thing about the alias, one word.
    public enum Status: String, Decodable, Equatable, Sendable {
        case ok, missing, broken, stale, foreign, shadowed, invalid, unknown

        public init(from decoder: Decoder) throws {
            let raw = try decoder.singleValueContainer().decode(String.self)
            self = Status(rawValue: raw) ?? .unknown
        }

        /// Works as it should.
        public var isFine: Bool { self == .ok }
    }
}

/// `cc-moss alias ls --json`.
public struct AliasList: Decodable, Equatable, Sendable {
    public var schema: Int
    /// `~/.local/bin`.
    public var dir: String
    /// The `cc-moss` the links point at.
    public var entry: String
    public var dirOnPath: Bool
    public var aliases: [Alias]

    enum CodingKeys: String, CodingKey {
        case schema, dir, entry, aliases
        case dirOnPath = "dir_on_path"
    }

    public static let supportedSchema = 1

    public static func decode(_ data: Data) throws -> AliasList {
        let list = try JSONDecoder().decode(AliasList.self, from: data)
        if list.schema > supportedSchema { throw SnapshotError.newerSchema(list.schema) }
        return list
    }

    /// Whether saving `name` would replace another alias: one with that name
    /// exists and it is not the row being edited (`editing`, nil when adding).
    public func replaces(_ name: String, editing: String?) -> Bool {
        name != editing && aliases.contains { $0.name == name }
    }

    /// The aliases whose name or command contains every word of `query`.
    public func filtered(_ query: String) -> [Alias] {
        let words = query.lowercased().split(whereSeparator: \.isWhitespace)
        guard !words.isEmpty else { return aliases }
        return aliases.filter { a in
            let text = "\(a.name) \(a.command)".lowercased()
            return words.allSatisfy { text.contains($0) }
        }
    }
}

/// Why cc-moss said no to `alias add` or `alias rm`.
public struct AliasFailure: Error, Equatable, Sendable {
    /// `bad_name`, `bad_command`, `exists` (`--force` helps), `foreign`,
    /// `not_found`, `config` or `error`.
    public var code: String
    public var message: String

    public init(code: String, message: String) {
        self.code = code
        self.message = message
    }

    /// The name is already a command elsewhere; adding with force is possible.
    public var canForce: Bool { code == "exists" }

    /// The app's own question: the name belongs to another alias.
    public static func taken(_ name: String) -> AliasFailure {
        AliasFailure(code: "taken",
                     message: L.t("There is already an alias called `\(name)`. Replace it?",
                                  "已经有一个叫 `\(name)` 的别名了，要替换它吗？"))
    }

    /// Saving again with consent replaces the other alias.
    public var canReplace: Bool { code == "taken" }
}

/// The argument lists the app hands cc-moss. Names and commands always come
/// after `--`, so nothing typed into a field is ever read as an option, and
/// the command is one argument: cc-moss splits it with its own tokenizer,
/// never a shell.
public enum AliasArgs {
    public static func list() -> [String] {
        ["alias", "ls", "--json"]
    }

    public static func add(name: String, command: String, force: Bool = false) -> [String] {
        ["alias", "add", "--json"] + (force ? ["--force"] : []) + ["--", name, command]
    }

    public static func remove(name: String) -> [String] {
        ["alias", "rm", "--json", "--", name]
    }
}

/// The reply to `alias add` / `alias rm`.
struct AliasReply: Decodable {
    var ok: Bool
    var code: String?
    var error: String?
    var alias: Alias?
}

extension MossClient {
    public func aliases() async throws -> AliasList {
        let r = try await raw(AliasArgs.list())
        guard r.status == 0 else {
            throw Failure.command(firstLine(r.stderr) ?? L.t("cc-moss exited with \(r.status)", "cc-moss 退出码 \(r.status)"))
        }
        do {
            return try AliasList.decode(r.stdout)
        } catch SnapshotError.newerSchema(let n) {
            throw Failure.newerSchema(n)
        } catch {
            throw Failure.unreadable("\(error)")
        }
    }

    /// Add `name`, or change what it runs. Throws `AliasFailure` when cc-moss refuses.
    @discardableResult
    public func addAlias(name: String, command: String, force: Bool = false) async throws -> Alias {
        let reply = try await aliasReply(AliasArgs.add(name: name, command: command, force: force))
        return reply.alias ?? Alias(name: name, command: command)
    }

    public func removeAlias(name: String) async throws {
        _ = try await aliasReply(AliasArgs.remove(name: name))
    }

    /// A new name for an alias: add the new one first, so a refused name
    /// leaves the old alias as it was, then remove the old one.
    @discardableResult
    public func renameAlias(from old: String, to new: String, command: String, force: Bool = false) async throws -> Alias {
        let added = try await addAlias(name: new, command: command, force: force)
        if old != new { try await removeAlias(name: old) }
        return added
    }

    private func raw(_ args: [String]) async throws -> CommandResult {
        do {
            return try await runner.run(executable, args, timeout: timeout)
        } catch RunError.timedOut {
            throw Failure.timedOut
        }
    }

    private func aliasReply(_ args: [String]) async throws -> AliasReply {
        let r = try await raw(args)
        if let reply = try? JSONDecoder().decode(AliasReply.self, from: r.stdout) {
            if reply.ok, r.status == 0 { return reply }
            throw AliasFailure(code: reply.code ?? "error",
                               message: reply.error ?? L.t("cc-moss said no.", "cc-moss 没同意。"))
        }
        throw Failure.command(firstLine(r.stderr) ?? firstLine(String(decoding: r.stdout, as: UTF8.self)) ?? "exit \(r.status)")
    }
}
