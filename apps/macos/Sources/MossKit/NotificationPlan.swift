import Foundation

/// The session events worth telling you about (A9). Everything else in the
/// stream (started, working, renamed, ended, closed) stays in the panel.
public enum AlertKind: String, CaseIterable, Sendable {
    /// A permission prompt, a question, a wait for input.
    case needsYou = "needs_you"
    /// The process died without anyone ending it.
    case interrupted
    /// A turn is over. Every turn makes one, so it is off by default.
    case finished
    /// An interrupted session is back.
    case restored

    public var defaultOn: Bool { self != .finished }

    /// What the alert offers; clicking the alert itself does the same.
    public var action: AlertAction { self == .interrupted ? .restore : .open }
}

/// The one thing an alert offers, in the system notification and in Mo's bubble alike.
public enum AlertAction: String, Sendable {
    /// Open the session in the terminal.
    case open
    /// Bring an interrupted session back.
    case restore

    public var title: String {
        switch self {
        case .open: L.t("Open", "打开")
        case .restore: L.t("Bring Back", "找回")
        }
    }
}

/// Which kinds you want to hear about. Kept in UserDefaults; a kind never
/// touched keeps its default.
public struct AlertToggles: Equatable, Sendable {
    private var on: Set<AlertKind>

    public init(on: Set<AlertKind> = Set(AlertKind.allCases.filter(\.defaultOn))) {
        self.on = on
    }

    public static func key(_ kind: AlertKind) -> String { "alert.\(kind.rawValue)" }

    public init(defaults: UserDefaults) {
        on = Set(AlertKind.allCases.filter { (defaults.object(forKey: Self.key($0)) as? Bool) ?? $0.defaultOn })
    }

    public func isOn(_ kind: AlertKind) -> Bool { on.contains(kind) }

    public mutating func set(_ kind: AlertKind, _ value: Bool) {
        if value { on.insert(kind) } else { on.remove(kind) }
    }
}

/// One thing worth telling you, made from one event. `id` is the event's
/// sequence number, which is what the router remembers.
public struct SessionAlert: Equatable, Sendable {
    public var id: Int
    public var slotId: String
    public var kind: AlertKind
    public var name: String?
    /// For `needsYou`: `permission`, `question` or `input`.
    public var reason: String?
    public var at: Date

    public init(id: Int, slotId: String, kind: AlertKind, name: String? = nil, reason: String? = nil, at: Date) {
        self.id = id
        self.slotId = slotId
        self.kind = kind
        self.name = name
        self.reason = reason
        self.at = at
    }
}

/// What a notification says and offers.
public struct AlertNote: Equatable, Sendable {
    /// Unique per alert, so the system never replaces one with another.
    public var id: String
    public var title: String
    public var body: String
    public var action: AlertAction
    public var slotId: String
    /// Notifications of one session stack together.
    public var thread: String
}

/// Turns new events into alerts: drops the kinds you switched off and folds
/// repeats of the same kind from the same session within 30 seconds into the first.
public struct NotificationPlan: Sendable {
    public static let mergeWindow: TimeInterval = 30

    private struct Key: Hashable {
        var slotId: String
        var kind: AlertKind
    }

    private var lastAlerted: [Key: Date] = [:]

    public init() {}

    /// Events arrive in order; `now` stands in for events without a time.
    public mutating func alerts(from events: [SessionEvent], toggles: AlertToggles, now: Date) -> [SessionAlert] {
        var out: [SessionAlert] = []
        for event in events.sorted(by: { $0.seq < $1.seq }) {
            guard let kind = AlertKind(rawValue: event.kind), toggles.isOn(kind) else { continue }
            let at = event.at ?? now
            let key = Key(slotId: event.slotId, kind: kind)
            if let last = lastAlerted[key], at.timeIntervalSince(last) < Self.mergeWindow, at >= last { continue }
            lastAlerted[key] = at
            out.append(SessionAlert(id: event.seq, slotId: event.slotId, kind: kind, name: event.name, reason: event.reason, at: at))
        }
        // Forget what can no longer merge.
        lastAlerted = lastAlerted.filter { now.timeIntervalSince($0.value) < Self.mergeWindow }
        return out
    }

    /// The notification for `alert`; `title` is the session's display name.
    /// The wording never promises "right now": a permission prompt reaches
    /// cc-moss some seconds after it appears.
    public static func note(for alert: SessionAlert, title: String) -> AlertNote {
        let heading: String
        let body: String
        switch alert.kind {
        case .needsYou:
            heading = L.t("\(title) needs you", "\(title) 需要你确认")
            switch alert.reason {
            case "question":
                body = L.t("It has a question for you.", "它有个问题想问你。")
            case "input":
                body = L.t("It's waiting for your reply.", "它在等你回话。")
            default:
                body = L.t("It wants to do something and is waiting for your OK.", "它想做一件事，等你点头。")
            }
        case .interrupted:
            heading = L.t("\(title) stopped", "\(title) 断了")
            body = L.t("The process is gone, the conversation isn't. Bring it back to carry on.",
                       "进程没了，对话还在。找回来就能接着聊。")
        case .finished:
            heading = L.t("\(title) is done for now", "\(title) 这一轮做完了")
            body = L.t("Your move.", "轮到你了。")
        case .restored:
            heading = L.t("\(title) is back", "\(title) 找回来了")
            body = L.t("Right where you left off.", "从上次停下的地方接着来。")
        }
        return AlertNote(id: "cc-moss.\(alert.id)", title: heading, body: body, action: alert.kind.action,
                         slotId: alert.slotId, thread: alert.slotId)
    }
}

/// Whether macOS lets cc-moss post notifications.
public enum NotificationAuth: Equatable, Sendable {
    /// Not looked yet.
    case unknown
    /// Never asked: the first alert that needs it asks.
    case notDetermined
    case allowed
    /// You said no, or turned it off in System Settings.
    case denied
    /// macOS refuses to ask at all: the app isn't in an Applications folder
    /// (notes/probes.md Q6), or it has no bundle.
    case unavailable
}

/// The system's notification center, behind a seam so the authorization
/// flow can be tested without a real app bundle.
@MainActor public protocol Notifier: AnyObject {
    func authorization() async -> NotificationAuth
    /// Shows macOS's prompt; answers what you chose.
    func requestAuthorization() async -> NotificationAuth
    /// True when the system took it.
    func post(_ note: AlertNote) async -> Bool
}

/// Asks for permission the first time a notification is due (not at
/// launch), at most once per run, and posts only when allowed.
@MainActor public final class NotificationGate {
    public private(set) var auth: NotificationAuth = .unknown {
        didSet { if auth != oldValue { onChange?(auth) } }
    }
    public var onChange: ((NotificationAuth) -> Void)?

    private let notifier: Notifier
    private var asked = false

    public init(notifier: Notifier) {
        self.notifier = notifier
    }

    /// Look at the current permission, for the settings page.
    public func refresh() async {
        auth = await notifier.authorization()
    }

    /// From the settings page: ask now instead of waiting for the first alert.
    public func requestNow() async {
        asked = true
        auth = await notifier.requestAuthorization()
    }

    /// Post `note` if macOS allows it, asking first if it never has. False
    /// when it was not posted; `auth` then says why.
    @discardableResult
    public func deliver(_ note: AlertNote) async -> Bool {
        var status = await notifier.authorization()
        if status == .notDetermined, !asked {
            asked = true
            status = await notifier.requestAuthorization()
        }
        auth = status
        guard status == .allowed else { return false }
        return await notifier.post(note)
    }
}
