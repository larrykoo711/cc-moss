import Foundation

/// Finds the `cc-moss` binary. Apps started from Finder or at login get a
/// bare PATH, so the usual install places are searched by name.
public struct Locator: Sendable {
    public var home: String
    /// A path the user set in the app's settings; wins when executable.
    public var override: String?
    public var isExecutable: @Sendable (String) -> Bool

    public init(
        home: String = NSHomeDirectory(),
        override: String? = nil,
        isExecutable: @escaping @Sendable (String) -> Bool = { FileManager.default.isExecutableFile(atPath: $0) }
    ) {
        self.home = home
        self.override = override
        self.isExecutable = isExecutable
    }

    /// Where to look, in order.
    public var candidates: [String] {
        var list: [String] = []
        if let override, !override.isEmpty { list.append(override) }
        list += [
            "/opt/homebrew/bin/cc-moss",
            "/usr/local/bin/cc-moss",
            home + "/.cargo/bin/cc-moss",
            home + "/.local/bin/cc-moss",
        ]
        return list
    }

    public func locate() -> String? {
        candidates.first(where: isExecutable)
    }
}
