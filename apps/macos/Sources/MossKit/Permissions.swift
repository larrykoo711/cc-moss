import Foundation

/// Where macOS stands on one permission cc-moss asks for.
public enum Grant: Equatable, Sendable {
    case allowed
    /// You said no, or switched it off in System Settings.
    case denied
    /// Never asked: macOS asks the first time it's used.
    case notAsked
    /// macOS can't say right now (Automation can only be checked while the
    /// other app is running).
    case unknown

    /// What `AEDeterminePermissionToAutomateTarget` returned, asked without
    /// prompting.
    public init(automationStatus status: Int32) {
        switch status {
        case 0: self = .allowed
        case -1743: self = .denied // errAEEventNotPermitted
        case -1744: self = .notAsked // errAEEventWouldRequireUserConsent
        default: self = .unknown // -600 procNotFound: not running
        }
    }
}

/// An app cc-moss sends Apple Events to, which macOS lists under
/// Privacy & Security › Automation.
public enum AutomationTarget: Equatable, Hashable, Sendable {
    /// The terminal sessions open in.
    case terminal(Terminal)
    /// Presses ⌘T in Terminal, for a new tab.
    case systemEvents

    public var bundleID: String {
        switch self {
        case let .terminal(t): t.bundleID
        case .systemEvents: "com.apple.systemevents"
        }
    }
}

/// Which permissions the current settings rely on, so the app only asks for
/// (and only warns about) what is actually used.
public enum PermissionNeeds {
    /// Apple Events cc-moss sends to open a session in `terminal`; see
    /// `LaunchPlan`: kitty, Alacritty, WezTerm and Ghostty before 1.3 are
    /// started from the command line and need none.
    public static func automation(for terminal: InstalledTerminal, style: OpenStyle) -> [AutomationTarget] {
        switch terminal.terminal {
        case .terminal:
            style == .tab ? [.terminal(.terminal), .systemEvents] : [.terminal(.terminal)]
        case .iterm2:
            [.terminal(.iterm2)]
        case .ghostty:
            Version.atLeast(terminal.version, [1, 3]) ? [.terminal(.ghostty)] : []
        case .wezterm, .kitty, .alacritty:
            []
        }
    }

    /// Accessibility is what lets a double tap be heard in other apps, and
    /// what lets cc-moss press ⌘T in Terminal.
    public static func accessibility(shortcut: PanelShortcut, terminal: Terminal?, style: OpenStyle) -> Bool {
        if shortcut.tapKey != nil { return true }
        return terminal == .terminal && style == .tab
    }
}

public enum PermissionSummary {
    /// How many permissions need you: switched off while something uses them.
    /// Not asked yet doesn't count; macOS asks when it's first needed.
    public static func attention(accessibility: Grant, accessibilityNeeded: Bool,
                                 notifications: NotificationAuth, automation: [Grant]) -> Int {
        var n = 0
        if accessibilityNeeded, accessibility != .allowed { n += 1 }
        if notifications == .denied { n += 1 }
        n += automation.filter { $0 == .denied }.count
        return n
    }
}

/// Remembers that Accessibility was once allowed. The app is signed ad hoc,
/// so macOS ties the permission to one build: after an update it silently
/// stops counting. Seeing it gone after it was there is how we know to say so.
public struct AccessibilityMemory {
    static let key = "accessibilityWasAllowed"
    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    public func record(trusted: Bool) {
        if trusted { defaults.set(true, forKey: Self.key) }
    }

    /// It was allowed before, isn't now, and something still needs it.
    public func lost(trusted: Bool, needed: Bool) -> Bool {
        !trusted && needed && defaults.bool(forKey: Self.key)
    }

    /// "Later": stop reminding until it's allowed again.
    public func dismiss() {
        defaults.removeObject(forKey: Self.key)
    }
}
