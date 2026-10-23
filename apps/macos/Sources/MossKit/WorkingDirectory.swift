import Foundation

/// Optional CLI capabilities are positive evidence; missing or newer versions stay unsupported.
public struct CLICapabilities: Decodable, Equatable, Sendable {
    public var workingDirectory: Int?
    public var newReceipt: Int?
    /// `restore --exact-slot-id`: restores by slot and full UUID, never by name.
    public var exactRestore: Int?
    public init(workingDirectory: Int? = nil, newReceipt: Int? = nil, exactRestore: Int? = nil) {
        self.workingDirectory = workingDirectory
        self.newReceipt = newReceipt
        self.exactRestore = exactRestore
    }
    public var checksWorkingDirectory: Bool { workingDirectory == 1 }
    public var returnsNewReceipt: Bool { newReceipt == 1 }
    public var restoresExactly: Bool { exactRestore == 1 }

    enum CodingKeys: String, CodingKey { case workingDirectory, newReceipt, exactRestore }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        workingDirectory = try? c.decodeIfPresent(Int.self, forKey: .workingDirectory)
        newReceipt = try? c.decodeIfPresent(Int.self, forKey: .newReceipt)
        exactRestore = try? c.decodeIfPresent(Int.self, forKey: .exactRestore)
    }
}

public enum WorkingDirectoryReason: String, Decodable, Equatable, Sendable {
    case forbidden = "cwd_forbidden"
    case missing = "cwd_missing"
    case notDirectory = "cwd_not_directory"
    case unreadable = "cwd_unreadable"
    case unavailable = "cwd_unavailable"
    case invalid = "cwd_invalid"
    case changed = "cwd_changed"
    case other

    public init(from decoder: Decoder) throws {
        self = Self(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .other
    }

    public var explanation: String {
        switch self {
        case .forbidden: L.t("This location can't be used as a working directory. Choose a project folder.", "此位置不能作为会话工作目录，请选择项目文件夹。")
        case .missing: L.t("The working directory no longer exists.", "工作目录已不存在。")
        case .notDirectory: L.t("The working directory must be a folder.", "工作目录必须是文件夹。")
        case .unreadable: L.t("The working directory can't be accessed. Check its permissions.", "无法访问工作目录，请检查权限。")
        case .unavailable: L.t("The working directory can't be checked right now. Try again later.", "暂时无法检查工作目录，请稍后重试。")
        case .invalid: L.t("The working directory path isn't valid.", "工作目录路径无效。")
        case .changed: L.t("The working directory changed. Choose it again before starting.", "工作目录已变化，请重新选择后再启动。")
        case .other: L.t("The working directory wasn't approved.", "工作目录尚未通过检查。")
        }
    }
}

/// Returned by check-directory, or without path in a session snapshot.
public struct WorkingDirectoryCheck: Decodable, Equatable, Sendable {
    public var path: String?
    public var ok: Bool
    public var canonical: String?
    public var reason: WorkingDirectoryReason?
    public var message: String?
    public var sourceResolved: Bool

    public var approvedForNewSession: Bool {
        ok && sourceResolved && canonical.map { $0.hasPrefix("/") && $0.rangeOfCharacter(from: .controlCharacters) == nil } == true
    }

    public var explanation: String? {
        guard !ok else { return nil }
        return reason?.explanation ?? message ?? WorkingDirectoryReason.unavailable.explanation
    }
}

/// A response is useful only for the exact selection request that is still current.
public struct WorkingDirectorySelection: Equatable, Sendable {
    public struct Ticket: Equatable, Sendable {
        public let id: UUID
        public let path: String
    }
    public private(set) var pending: Ticket?
    public private(set) var approved: WorkingDirectoryCheck?
    public private(set) var error: String?
    public init() {}
    public var canonical: String? { approved?.approvedForNewSession == true ? approved?.canonical : nil }

    public mutating func begin(_ path: String) -> Ticket {
        let ticket = Ticket(id: UUID(), path: path)
        pending = ticket
        approved = nil
        error = nil
        return ticket
    }

    @discardableResult public mutating func complete(_ ticket: Ticket, check: WorkingDirectoryCheck) -> Bool {
        guard pending == ticket, check.path == ticket.path else { return false }
        pending = nil
        if check.approvedForNewSession { approved = check; error = nil }
        else { approved = nil; error = check.explanation ?? WorkingDirectoryReason.unavailable.explanation }
        return true
    }

    public mutating func fail(_ ticket: Ticket, message: String) {
        guard pending == ticket else { return }
        pending = nil
        approved = nil
        error = message
    }

    public mutating func cancel() { pending = nil; approved = nil; error = nil }
}

public enum WorkingDirectoryPolicy {
    public static var updateRequired: String {
        L.t("Update cc-moss in Terminal before starting a session. This version can't check working directories.",
            "请先更新终端里的 cc-moss；当前版本无法检查工作目录。")
    }
}

extension MossClient {
    /// The backend owns all path policy; no filesystem blacklist or synchronous probe lives in the app.
    public func checkDirectories(_ paths: [String], capabilities: CLICapabilities) async throws -> [WorkingDirectoryCheck] {
        guard capabilities.checksWorkingDirectory else { throw Failure.command(WorkingDirectoryPolicy.updateRequired) }
        guard !paths.isEmpty else { return [] }
        guard paths.count <= 64 else { throw Failure.command("At most 64 working directories can be checked at once.") }
        let data = try await plainData(["check-directory", "--json", "--"] + paths)
        struct Reply: Decodable { var schema: Int; var results: [WorkingDirectoryCheck] }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let reply: Reply
        do { reply = try decoder.decode(Reply.self, from: data) }
        catch { throw Failure.unreadable("Invalid working directory response.") }
        guard reply.schema <= Snapshot.supportedSchema else { throw Failure.newerSchema(reply.schema) }
        guard reply.schema == 1, reply.results.count == paths.count,
              zip(reply.results, paths).allSatisfy({ $0.0.path == $0.1 }) else {
            throw Failure.unreadable("Working directory results did not match the requested paths.")
        }
        return reply.results
    }
}
