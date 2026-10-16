import AppKit
import MossKit
import Observation
import UserNotifications

/// macOS's Notification Center. Only made inside a real app bundle:
/// `UNUserNotificationCenter` traps without one (`--render`, `swift run`).
@MainActor
final class SystemNotifier: NSObject, Notifier, UNUserNotificationCenterDelegate {
    /// You clicked a notification or one of its buttons.
    var onAction: ((AlertAction, String) -> Void)?

    private let center = UNUserNotificationCenter.current()
    private nonisolated static let slotKey = "slot"

    override init() {
        super.init()
        center.delegate = self
        center.setNotificationCategories(Set([AlertAction.open, .restore].map { action in
            UNNotificationCategory(
                identifier: Self.category(action),
                actions: [UNNotificationAction(identifier: action.rawValue, title: action.title)],
                intentIdentifiers: []
            )
        }))
    }

    static func category(_ action: AlertAction) -> String { "cc-moss.\(action.rawValue)" }

    /// Only an app bundle can talk to Notification Center.
    static var available: Bool { Bundle.main.bundleIdentifier != nil && Bundle.main.bundleURL.pathExtension == "app" }

    func authorization() async -> NotificationAuth {
        let settings = await center.notificationSettings()
        switch settings.authorizationStatus {
        case .notDetermined: return .notDetermined
        case .denied: return .denied
        case .authorized, .provisional, .ephemeral: return .allowed
        @unknown default: return .denied
        }
    }

    func requestAuthorization() async -> NotificationAuth {
        do {
            return try await center.requestAuthorization(options: [.alert, .sound]) ? .allowed : .denied
        } catch {
            // UNErrorDomain 1: "Notifications are not allowed for this
            // application", which is what an app outside /Applications gets.
            return .unavailable
        }
    }

    func post(_ note: AlertNote) async -> Bool {
        let content = UNMutableNotificationContent()
        content.title = note.title
        content.body = note.body
        content.sound = .default
        content.threadIdentifier = note.thread
        content.categoryIdentifier = Self.category(note.action)
        content.userInfo = [Self.slotKey: note.slotId]
        do {
            try await center.add(UNNotificationRequest(identifier: note.id, content: content, trigger: nil))
            return true
        } catch {
            return false
        }
    }

    // A menu bar app counts as frontmost while its settings window is open;
    // show the banner then too.
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                            withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                            withCompletionHandler completionHandler: @escaping () -> Void) {
        let content = response.notification.request.content
        let slot = content.userInfo[Self.slotKey] as? String
        let category = content.categoryIdentifier
        let button = response.actionIdentifier
        Task { @MainActor in
            defer { completionHandler() }
            guard let slot, button != UNNotificationDismissActionIdentifier else { return }
            // A click on the notification itself does what its button does.
            let action = AlertAction(rawValue: button) ?? (category == Self.category(.restore) ? .restore : .open)
            self.onAction?(action, slot)
        }
    }
}

/// What the settings page shows about Notification Center, and its one button.
@MainActor @Observable
final class NotificationStatus {
    static let shared = NotificationStatus()

    var auth: NotificationAuth = .unknown
    @ObservationIgnored var request: () async -> Void = {}
    @ObservationIgnored var refresh: () async -> Void = {}

    /// System Settings › Notifications, at cc-moss's entry when it has one.
    static func openSystemSettings() {
        let id = Bundle.main.bundleIdentifier.map { "?id=\($0)" } ?? ""
        if let url = URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension\(id)") {
            NSWorkspace.shared.open(url)
        }
    }
}

/// Turns session events into at most one alert each, in one place (A23):
/// Mo when it has the stage, Notification Center otherwise, nowhere when the
/// panel already shows it or you asked for quiet.
@MainActor
final class AlertCenter {
    private let model: AppModel
    private let gate: NotificationGate?
    private let defaults: UserDefaults
    private let context: () -> AlertContext
    private let toPet: (SessionAlert) -> Void
    private var plan = NotificationPlan()
    private var router = AlertRouter()

    /// `notifier` is nil where Notification Center can't be used; alerts
    /// due there are then dropped (the panel and the icon still show them).
    init(model: AppModel, notifier: Notifier?, status: NotificationStatus? = nil, defaults: UserDefaults = .standard,
         context: @escaping () -> AlertContext, toPet: @escaping (SessionAlert) -> Void) {
        self.model = model
        self.defaults = defaults
        self.context = context
        self.toPet = toPet
        let status = status ?? .shared
        let gate = notifier.map(NotificationGate.init)
        self.gate = gate
        if let gate {
            gate.onChange = { [weak status] in status?.auth = $0 }
            status.request = { await gate.requestNow() }
            status.refresh = { await gate.refresh() }
            Task { await gate.refresh() }
        } else {
            status.auth = .unavailable
        }
    }

    func handle(_ events: [SessionEvent]) {
        let alerts = plan.alerts(from: events, toggles: AlertToggles(defaults: defaults), now: Date())
        guard !alerts.isEmpty else { return }
        let ctx = context()
        for alert in alerts {
            switch router.route(alert, in: ctx) {
            case .pet:
                toPet(alert)
            case .notification:
                let note = NotificationPlan.note(for: alert, title: title(alert))
                if let gate { Task { await gate.deliver(note) } }
            case .none:
                break
            }
        }
    }

    /// The alert's main action, the same from a notification and from Mo's
    /// bubble: open the session in the terminal; if it can't be opened from
    /// here, or it needs bringing back, show it in the panel, where the
    /// panel's own main action (open, bring back) is one key away.
    static func perform(_ action: AlertAction, slot: String, model: AppModel, showPanel: () -> Void) {
        let session = model.sessions.first { $0.id == slot }
        if action == .open, let session, session.inTmux, session.state != .interrupted, model.open(session) { return }
        model.query = ""
        model.stateFilter = nil
        if session != nil { model.selectedID = slot }
        showPanel()
    }

    /// The name the panel shows for the session.
    private func title(_ alert: SessionAlert) -> String {
        if let s = model.sessions.first(where: { $0.id == alert.slotId }) { return s.title }
        if let name = alert.name, !name.isEmpty { return name }
        return String(alert.slotId.prefix(8))
    }
}
