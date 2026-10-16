import Foundation

/// Where alerts go (Settings › Notifications), A23.
public enum AlertMode: String, CaseIterable, Sendable {
    /// Mo when it's on the desktop and you can see it; Notification Center otherwise.
    case auto
    case notificationsOnly = "notifications"
    /// Mo only, while Mo is switched on.
    case petOnly = "pet"

    public static let key = "alertMode"

    public init(defaults: UserDefaults) {
        self = defaults.string(forKey: Self.key).flatMap(AlertMode.init(rawValue:)) ?? .auto
    }
}

/// The one place an alert ends up.
public enum AlertDestination: Equatable, Sendable {
    case notification
    case pet
    /// Nowhere: already told, the panel shows it, or do not disturb.
    case none
}

/// What the router needs to know about the world at the moment an alert arrives.
public struct AlertContext: Equatable, Sendable {
    public var mode: AlertMode
    /// Mo is switched on in Settings.
    public var petEnabled: Bool
    /// Mo is on screen where you can see it: not tucked away, covered by a
    /// full-screen app, or behind a locked or sleeping screen.
    public var petVisible: Bool
    /// The panel is open, and it already shows every session's state.
    public var panelOpen: Bool
    public var doNotDisturb: Bool

    public init(mode: AlertMode = .auto, petEnabled: Bool = false, petVisible: Bool = false,
                panelOpen: Bool = false, doNotDisturb: Bool = false) {
        self.mode = mode
        self.petEnabled = petEnabled
        self.petVisible = petVisible && petEnabled
        self.panelOpen = panelOpen
        self.doNotDisturb = doNotDisturb
    }
}

/// One alert, one place, once (A23). The Notification Center (T2.3) and
/// Mo's bubbles (T3.3) both ask this router; neither decides on its own.
public struct AlertRouter: Sendable {
    /// Alerts already decided, whatever the answer was: an alert that went
    /// nowhere because the panel was open is not sent later either.
    private var decided: Set<Int> = []
    private static let memory = 1000

    public init() {}

    public static func destination(for alert: SessionAlert, in context: AlertContext) -> AlertDestination {
        if context.doNotDisturb || context.panelOpen { return .none }
        switch context.mode {
        case .notificationsOnly:
            return .notification
        case .petOnly:
            // Mo switched off can't take anything: fall back so nothing is lost.
            return context.petEnabled ? .pet : .notification
        case .auto:
            return context.petVisible ? .pet : .notification
        }
    }

    /// Decide once. Asking again about the same alert answers `.none`, so a
    /// place that becomes available later never gets a second copy.
    public mutating func route(_ alert: SessionAlert, in context: AlertContext) -> AlertDestination {
        guard decided.insert(alert.id).inserted else { return .none }
        if decided.count > Self.memory, let oldest = decided.min() { decided.remove(oldest) }
        return Self.destination(for: alert, in: context)
    }
}
