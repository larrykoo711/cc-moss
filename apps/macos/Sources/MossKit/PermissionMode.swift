import Foundation

/// How new sessions start, shared with the CLI's `[claude] permission_mode`:
/// `cc-moss new` adds bypass unless this says `default`. The app only reads
/// and writes the setting; the CLI decides the command line.
public enum PermissionMode: String, Sendable, Equatable {
    case bypass
    case `default`

    public static let configKey = "claude.permission_mode"

    /// What the CLI printed; anything but `default` is bypass, as the CLI reads it.
    public init(config: String?) {
        let value = config?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        self = value == PermissionMode.default.rawValue ? .default : .bypass
    }

    public init(bypass: Bool) {
        self = bypass ? .bypass : .default
    }

    public var isBypass: Bool { self == .bypass }
    public var configValue: String { rawValue }
}

extension MossClient {
    /// `cc-moss config get claude.permission_mode`.
    public func permissionMode() async throws -> PermissionMode {
        PermissionMode(config: try await config(PermissionMode.configKey))
    }

    /// `cc-moss config set claude.permission_mode …`.
    public func setPermissionMode(_ mode: PermissionMode) async throws {
        try await setConfig(PermissionMode.configKey, mode.configValue)
    }
}
