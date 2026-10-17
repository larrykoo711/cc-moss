import Foundation

/// A terminal app cc-moss knows how to open a session in.
public enum Terminal: String, CaseIterable, Codable, Sendable {
    case terminal, ghostty, iterm2, wezterm, kitty, alacritty

    /// What a fresh install uses, whatever else is installed: the one that
    /// comes with macOS.
    public static let `default`: Terminal = .terminal

    public var bundleID: String {
        switch self {
        case .terminal: "com.apple.Terminal"
        case .ghostty: "com.mitchellh.ghostty"
        case .iterm2: "com.googlecode.iterm2"
        case .wezterm: "com.github.wez.wezterm"
        case .kitty: "net.kovidgoyal.kitty"
        case .alacritty: "org.alacritty"
        }
    }

    public var displayName: String {
        switch self {
        case .terminal: L.t("Terminal", "终端")
        case .ghostty: "Ghostty"
        case .iterm2: "iTerm2"
        case .wezterm: "WezTerm"
        case .kitty: "kitty"
        case .alacritty: "Alacritty"
        }
    }

    /// The one we suggest to people who haven't picked yet.
    public var isRecommended: Bool { self == .ghostty }

    /// Where to get it, for the "not installed" suggestion.
    public var downloadURL: URL? {
        switch self {
        case .ghostty: URL(string: "https://ghostty.org/download")
        default: nil
        }
    }

    /// Whether cc-moss can open a session as a tab in this terminal's window.
    /// Ghostty learnt it with scripting in 1.3; kitty needs remote control
    /// switched on and Alacritty has no tabs, so those always get a window.
    public func supportsTabs(version: String?) -> Bool {
        switch self {
        case .terminal, .iterm2, .wezterm: true
        case .ghostty: Version.atLeast(version, [1, 3])
        case .kitty, .alacritty: false
        }
    }
}

/// How a session opens: as a tab in the terminal's front window (what most
/// developer tools do, and the default) or in a window of its own.
public enum OpenStyle: String, CaseIterable, Codable, Sendable {
    case tab, window

    public static let `default`: OpenStyle = .tab
}

/// A supported terminal found on this Mac.
public struct InstalledTerminal: Equatable, Sendable {
    public var terminal: Terminal
    /// The `.app` bundle.
    public var appPath: String
    /// `CFBundleShortVersionString`, when the bundle says.
    public var version: String?

    public init(terminal: Terminal, appPath: String, version: String? = nil) {
        self.terminal = terminal
        self.appPath = appPath
        self.version = version
    }
}

/// Finds which supported terminals are installed. The lookups are injected,
/// so tests decide what "installed" means.
public struct TerminalCatalog: Sendable {
    /// Bundle id → path of the app, nil when not installed.
    public var appPath: @Sendable (String) -> String?
    /// App path → its short version string.
    public var version: @Sendable (String) -> String?

    public init(
        appPath: @escaping @Sendable (String) -> String?,
        version: @escaping @Sendable (String) -> String? = TerminalCatalog.bundleVersion
    ) {
        self.appPath = appPath
        self.version = version
    }

    @Sendable public static func bundleVersion(_ appPath: String) -> String? {
        Bundle(path: appPath)?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
    }

    /// Installed supported terminals, in `Terminal.allCases` order.
    public func installed() -> [InstalledTerminal] {
        Terminal.allCases.compactMap { find($0) }
    }

    public func find(_ terminal: Terminal) -> InstalledTerminal? {
        guard let path = appPath(terminal.bundleID) else { return nil }
        return InstalledTerminal(terminal: terminal, appPath: path, version: version(path))
    }

    /// What to open sessions in.
    public struct Choice: Equatable, Sendable {
        /// nil only when not even the system terminal can be found.
        public var installed: InstalledTerminal?
        /// The chosen terminal is gone and the system one stands in.
        public var fellBackFrom: Terminal?
    }

    /// The user's pick (nil when they never chose) if it is still installed,
    /// otherwise the system terminal.
    public func resolve(_ chosen: Terminal?) -> Choice {
        let wanted = chosen ?? .default
        if let hit = find(wanted) { return Choice(installed: hit, fellBackFrom: nil) }
        return Choice(installed: find(.default), fellBackFrom: wanted == .default ? nil : wanted)
    }
}

/// Versions such as `1.3.1`, `1.2.0-dev+abc`, compared number by number.
public enum Version {
    public static func atLeast(_ version: String?, _ minimum: [Int]) -> Bool {
        guard let version else { return false }
        let core = version.split(whereSeparator: { $0 == "-" || $0 == "+" || $0 == " " }).first ?? ""
        let parts = core.split(separator: ".").map { Int($0) }
        guard !parts.isEmpty, parts.allSatisfy({ $0 != nil }) else { return false }
        let nums = parts.map { $0! }
        for (i, need) in minimum.enumerated() {
            let have = i < nums.count ? nums[i] : 0
            if have != need { return have > need }
        }
        return true
    }
}
