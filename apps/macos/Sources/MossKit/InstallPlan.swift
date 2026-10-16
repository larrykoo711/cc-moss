import Foundation

/// What `cc-moss install --dry-run --json` says about each part: ready, or
/// what installing would change. The welcome's "terminal tools" step.
public struct InstallPlan: Decodable, Equatable, Sendable {
    public struct Part: Decodable, Equatable, Sendable, Identifiable {
        public enum State: String, Decodable, Sendable {
            case ready, changes, skipped, unsupported
        }

        public var part: String
        public var state: State
        public var summary: String?
        public var id: String { part }

        public init(part: String, state: State, summary: String?) {
            self.part = part
            self.state = state
            self.summary = summary
        }
    }

    public var schema: Int
    public var parts: [Part]

    public init(schema: Int, parts: [Part]) {
        self.schema = schema
        self.parts = parts
    }

    /// The parts the welcome shows, in this order (the app installs itself).
    public static let shown = ["hooks", "tmux", "starship", "shell"]

    public var visible: [Part] { Self.shown.compactMap { name in parts.first { $0.part == name } } }

    /// Nothing left to do.
    public var isReady: Bool { visible.allSatisfy { $0.state != .changes } }

    public static func decode(_ data: Data) throws -> InstallPlan {
        let plan = try JSONDecoder().decode(InstallPlan.self, from: data)
        if plan.schema > 1 { throw SnapshotError.newerSchema(plan.schema) }
        return plan
    }
}

extension MossClient {
    /// What installing would change; changes nothing.
    public func installPlan() async throws -> InstallPlan {
        let out = try await plain(["install", "--dry-run", "--json"], timeout: 30)
        do {
            return try InstallPlan.decode(Data(out.utf8))
        } catch {
            throw Failure.unreadable("\(error)")
        }
    }

    /// Install everything but the app itself (it is already running).
    public func installTools() async throws {
        _ = try await plain(["install", "--yes", "--skip", "app"], timeout: 120)
    }
}
