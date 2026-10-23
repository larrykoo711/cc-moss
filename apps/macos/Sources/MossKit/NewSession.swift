import Foundation

/// A fresh identity on every creation; only its receipt can identify the session to open.
public enum NewSession {
    /// What pressing "Create" leads to.
    public enum Plan: Equatable, Sendable {
        /// Run cc-moss with these arguments, then open the new session.
        case create([String])
        /// The name can't be used; why, in words.
        case invalid(String)
    }

    /// What the name field says while you type.
    public enum Hint: Equatable, Sendable {
        case free
        case running(Session)
        case interrupted(Session)
        case invalid(String)
    }

    /// Up to `limit` folders sessions worked in, most recent first.
    public static func recentProjects(_ sessions: [Session], limit: Int = 10) -> [String] {
        var seen = Set<String>()
        return sessions
            .sorted { $0.lastActiveAt > $1.lastActiveAt }
            .map(\.cwd)
            .filter { seen.insert($0).inserted }
            .prefix(limit)
            .map { $0 }
    }

    /// Settings › General: where new sessions start. Unset: the most recent folder.
    public static let workspaceKey = "newSessionWorkspace"

    /// The name the form suggests: the folder's name, like `cc-moss new` does.
    public static func defaultName(for directory: String) -> String {
        let last = (directory as NSString).lastPathComponent
        return last.isEmpty || last == "/" ? "session" : last
    }

    /// `name`, or the first of `name-2`, `name-3`… no open session holds
    /// (a closed one gives its name up), like Finder's "untitled 2".
    public static func freeName(_ name: String, sessions: [Session]) -> String {
        let base = name.trimmingCharacters(in: .whitespaces)
        let taken = Set(sessions.filter { $0.state != .closed }.compactMap(\.name))
        guard taken.contains(base) else { return base }
        var n = 2
        while taken.contains("\(base)-\(n)") { n += 1 }
        return "\(base)-\(n)"
    }

    /// Fresh creation always asks for a structured identity receipt; restoration is a separate command.
    public static func arguments(name: String, directory: String) -> [String] {
        ["new", name, "-C", directory, "--detach", "--json"]
    }

    /// Why `name` can't be used, or nil.
    public static func problem(with name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return L.t("Give it a name.", "给它起个名字。") }
        // `new -x` would read as an option, and `new -- x` hands x to Claude.
        if trimmed.hasPrefix("-") { return L.t("A name can't start with “-”.", "名字不能以「-」开头。") }
        if trimmed.unicodeScalars.contains(where: { $0.properties.generalCategory == .control }) {
            return L.t("No control characters, please.", "名字里不能有控制字符。")
        }
        return nil
    }

    /// The open session already called `name`, if any.
    public static func hint(name: String, sessions: [Session]) -> Hint {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if let problem = problem(with: trimmed) { return .invalid(problem) }
        let same = sessions.filter { $0.name == trimmed && $0.state != .closed }
        if let running = same.first(where: { $0.state == .working || $0.state == .waiting }) {
            return .running(running)
        }
        if let stopped = same.first(where: { $0.state == .interrupted }) {
            return .interrupted(stopped)
        }
        return .free
    }

    public static func plan(name: String, directory: String, sessions: [Session]) -> Plan {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if let why = problem(with: trimmed) { return .invalid(why) }
        return .create(arguments(name: trimmed, directory: directory))
    }
}

/// A launch receipt identifies this request even before its first hook arrives.
public struct NewSessionReceipt: Decodable, Equatable, Sendable {
    public var result: String
    public var sessionId: String
    public var name: String
    public var cwd: String
    public var slotId: String?
    public var tmux: Session.Tmux?
    public var unconfirmed: Bool { result == "unconfirmed" }

    public static func decode(_ data: Data) throws -> Self {
        struct Wire: Decodable { var schema: Int }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let head = try decoder.decode(Wire.self, from: data)
        guard head.schema <= Snapshot.supportedSchema else { throw SnapshotError.newerSchema(head.schema) }
        let receipt = try decoder.decode(Self.self, from: data)
        guard head.schema == 1, ["starting", "unconfirmed"].contains(receipt.result),
              UUID(uuidString: receipt.sessionId) != nil, !receipt.name.isEmpty,
              receipt.cwd.hasPrefix("/"), receipt.cwd.rangeOfCharacter(from: .controlCharacters) == nil else {
            throw SnapshotError.invalid("Invalid new-session receipt.")
        }
        return receipt
    }

    public func registeredSession(in sessions: [Session]) -> Session? {
        let matching = sessions.filter {
            $0.sessionId == sessionId && (slotId == nil || $0.id == slotId)
                && $0.status == "running" && $0.inTmux && !$0.isArchived
        }
        return matching.count == 1 ? matching.first : nil
    }

    public func statusMessage(in sessions: [Session]) -> String {
        guard let registered = registeredSession(in: sessions) else { return waitingMessage }
        return L.t("\(registered.title) is registered and ready to open.", "\(registered.title) 已登记，可打开会话。")
    }

    public var waitingMessage: String {
        unconfirmed
            ? L.t("The launch result isn't confirmed. Following this session ID; nothing will be started again automatically.", "启动结果尚未确认，正在按会话编号跟踪，不会自动再次新建。")
            : L.t("\(name) is starting. Waiting for its session to register.", "\(name) 正在启动，等待会话登记。")
    }
}

extension MossClient {
    public func createNewSession(name: String, directory: String, capabilities: CLICapabilities) async throws -> NewSessionReceipt {
        guard capabilities.returnsNewReceipt && capabilities.checksWorkingDirectory else {
            throw Failure.command(L.t("Update cc-moss in Terminal before creating a session.", "请先更新终端里的 cc-moss，再新建会话。"))
        }
        let data = try await plainData(NewSession.arguments(name: name, directory: directory), timeout: 30)
        do { return try NewSessionReceipt.decode(data) }
        catch SnapshotError.newerSchema(let schema) { throw Failure.newerSchema(schema) }
        catch { throw Failure.unreadable("Invalid new-session receipt; do not retry automatically.") }
    }
}
