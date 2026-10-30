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
    /// You clicked a question's notification: its id.
    var onAsk: ((String) -> Void)?

    private let center = UNUserNotificationCenter.current()
    private nonisolated static let slotKey = "slot"
    private nonisolated static let askKey = "ask"
    private nonisolated static let askCategory = "cc-moss.ask"

    override init() {
        super.init()
        center.delegate = self
        var categories = Set([AlertAction.open, .restore].map { action in
            UNNotificationCategory(
                identifier: Self.category(action),
                actions: [UNNotificationAction(identifier: action.rawValue, title: action.title)],
                intentIdentifiers: []
            )
        })
        // A question opens the panel on its card; the answers are there.
        categories.insert(UNNotificationCategory(
            identifier: Self.askCategory,
            actions: [UNNotificationAction(identifier: "answer", title: L.t("Answer", "去回答"))],
            intentIdentifiers: []
        ))
        center.setNotificationCategories(categories)
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
        content.categoryIdentifier = note.askId == nil ? Self.category(note.action) : Self.askCategory
        content.userInfo = [Self.slotKey: note.slotId, Self.askKey: note.askId ?? ""]
        do {
            try await center.add(UNNotificationRequest(identifier: note.id, content: content, trigger: nil))
            return true
        } catch {
            return false
        }
    }

    func withdraw(_ ids: [String]) {
        center.removeDeliveredNotifications(withIdentifiers: ids)
        center.removePendingNotificationRequests(withIdentifiers: ids)
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
        let ask = (content.userInfo[Self.askKey] as? String).flatMap { $0.isEmpty ? nil : $0 }
        let category = content.categoryIdentifier
        let button = response.actionIdentifier
        Task { @MainActor in
            defer { completionHandler() }
            guard let slot, button != UNNotificationDismissActionIdentifier else { return }
            if let ask {
                self.onAsk?(ask)
                return
            }
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
    private var askAlerts = AskAlerts()
    private var mossSerial = 0

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
        let now = Date()
        let alerts = plan.alerts(from: events, toggles: AlertToggles(defaults: defaults), now: now)
        guard !alerts.isEmpty else { return }
        let ctx = context()
        // A question already told you about this session: no second alert.
        for alert in alerts where askAlerts.allows(alert, now: now) {
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

    /// Questions came and went (T1.4). New ones are told once, in one
    /// place, under the "Waiting for you" switch; gone ones are taken back,
    /// wherever they went (Mo lets go of its message by itself once the
    /// card is gone).
    func handleAsks(_ change: AskDesk.Change) {
        let now = Date()
        if AlertToggles(defaults: defaults).isOn(.needsYou), !change.arrived.isEmpty {
            let told = askAlerts.arrived(change.arrived, slot: { [model] in model.askSlot($0) },
                                         title: { [model] in model.askTitle($0) }, in: context(), now: now)
            deliver(told)
        }
        let back = askAlerts.gone(change.gone).filter { $0.destination == .notification }
        if let gate, !back.isEmpty {
            gate.withdraw(back.compactMap { $0.alert.askId.map(NotificationPlan.askNoteID) })
        }
    }

    /// Moss answered something you sent from the panel (T2.3): told once,
    /// where `MossReplyRouter` says (nowhere while you're on the Moss page).
    /// Several at once are one alert, for the newest; the rail's dot has the rest.
    func handleMossReplies(_ replies: [MossMessage]) {
        guard let reply = replies.last else { return }
        let now = Date()
        switch MossReplyRouter.destination(in: context(), onMossPage: model.page == .moss) {
        case .pet:
            mossSerial += 1
            // Ids below every event's sequence number, the questions' and the startup reminder's.
            toPet(MossReplyRouter.alert(for: reply, id: Int.min / 4 + mossSerial, at: now))
        case .notification:
            if let gate { Task { await gate.deliver(MossReplyRouter.note(for: reply)) } }
        case .none:
            break
        }
    }

    /// The panel closed with questions it showed still unanswered: tell
    /// them now (Mo, else a notification), so they don't wait unseen.
    func panelClosed() {
        guard AlertToggles(defaults: defaults).isOn(.needsYou) else { return }
        deliver(askAlerts.panelClosed(stillOpen: model.askDesk.open.map(\.ask), in: context(), now: Date()))
    }

    private func deliver(_ told: [AskAlerts.Told]) {
        for t in told {
            switch t.destination {
            case .pet:
                toPet(t.alert)
            case .notification:
                let note = NotificationPlan.askNote(for: t.alert, title: t.alert.name ?? "")
                if let gate { Task { await gate.deliver(note) } }
            case .none:
                break
            }
        }
    }

    /// After a restart or a crash (A8): the sessions that stopped while the
    /// app was away. One is an ordinary alert; several are one summary, so
    /// a reboot doesn't bury you in banners. Same rules as any alert: the
    /// "stopped" switch, quiet hours, an open panel, one place only.
    func remindMissed(_ reminder: StartupCheck.Reminder) {
        let ids: [String]
        switch reminder {
        case .none: return
        case let .one(id): ids = [id]
        case let .several(list): ids = list
        }
        guard AlertToggles(defaults: defaults).isOn(.interrupted) else { return }
        let now = Date()
        // Negative ids never meet a real event's sequence number.
        let alerts = ids.enumerated().map { i, id in
            SessionAlert(id: -(Int(now.timeIntervalSince1970) * 100 + i), slotId: id, kind: .interrupted,
                         name: model.session(id)?.title, at: now)
        }
        let ctx = context()
        switch router.route(alerts[0], in: ctx) {
        case .pet:
            alerts.forEach(toPet)
        case .notification:
            let note = alerts.count == 1
                ? NotificationPlan.note(for: alerts[0], title: title(alerts[0]))
                : NotificationPlan.missedNote(count: alerts.count, at: now)
            if let gate { Task { await gate.deliver(note) } }
        case .none:
            break
        }
    }

    /// The alert's main action, the same from a notification and from Mo's
    /// bubble: open the session in the terminal; if it can't be opened from
    /// here, or it needs bringing back, show it in the panel, where the
    /// panel's own main action (open, bring back) is one key away.
    static func perform(_ action: AlertAction, slot: String, model: AppModel, showPanel: @escaping @MainActor () -> Void) {
        // The startup summary: the panel, on the sessions that stopped,
        // where "Bring All Back" is one click.
        if slot == NotificationPlan.allInterrupted {
            model.query = ""
            model.stateFilter = .interrupted
            showPanel()
            return
        }
        // Moss answered: the panel, on the Moss page.
        if slot == MossReplyRouter.slot {
            model.showMoss()
            showPanel()
            return
        }
        // It has a question open: the panel, on that question, where it can be
        // answered without the terminal (T1.4).
        if action == .open, let card = model.openAsk(forSlot: slot) {
            model.focusAsk(card.id)
            showPanel()
            return
        }
        let session = model.session(slot)
        if action == .open, let session, session.canOpen, session.state != .interrupted {
            // Opens off the main thread; if it can't, the panel shows why.
            model.open(session) { opened in if !opened { showPanel() } }
            return
        }
        // Bring it back in one go (T1.3), and show the panel so its progress
        // and result are visible.
        if action == .restore, let session, session.state == .interrupted {
            model.restore(session)
        }
        model.query = ""
        model.stateFilter = nil
        if session != nil { model.selectedID = slot }
        showPanel()
    }

    /// The name the panel shows for the session.
    private func title(_ alert: SessionAlert) -> String {
        if let s = model.session(alert.slotId) { return s.title }
        if let name = alert.name, !name.isEmpty { return name }
        return String(alert.slotId.prefix(8))
    }
}
