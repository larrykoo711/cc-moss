import Foundation

/// Telling you about questions (T1.4), by the same rule as every alert
/// (A23): one place, once. A question goes where `AlertRouter` sends any
/// alert (Mo when it's in view, else Notification Center, nowhere while the
/// panel is open), is never told twice, and is taken back when it goes.
public struct AskAlerts: Sendable {
    /// One question's alert and where it went.
    public struct Told: Equatable, Sendable {
        public var alert: SessionAlert
        public var destination: AlertDestination
    }

    /// Questions already decided, whatever the answer was.
    private var told: [String: Told] = [:]
    /// Questions the open panel showed instead of telling you anywhere: told
    /// when the panel closes with them still unanswered.
    private var heldByPanel: [String: SessionAlert] = [:]
    /// The last "needs you" event alert per session, to keep the two
    /// sources (the event stream and the question list) from both speaking.
    private var eventAlerted: [String: Date] = [:]
    private var serial = 0

    /// An event alert and a question within this long are the same moment.
    public static let overlap: TimeInterval = 60

    public init() {}

    /// The questions seen for the first time: where each one goes. `slot`
    /// and `title` come from the panel's sessions.
    public mutating func arrived(_ asks: [Ask], slot: (Ask) -> String?, title: (Ask) -> String,
                                 in context: AlertContext, now: Date) -> [Told] {
        var out: [Told] = []
        for ask in asks where told[ask.id] == nil {
            serial += 1
            // Ids below every event's sequence number and every startup reminder's.
            let alert = SessionAlert(id: Int.min / 2 + serial, slotId: slot(ask) ?? ask.sessionId, kind: .needsYou,
                                     name: title(ask), reason: ask.kind == .permission ? "permission" : "question",
                                     at: now, askId: ask.id)
            var destination = AlertRouter.destination(for: alert, in: context)
            // The session's own "needs you" banner already went out for this
            // moment: no second banner. Mo still gets it, since Mo folds it into
            // what it holds for that session and can now offer the options.
            if destination == .notification, let last = eventAlerted[alert.slotId],
               now.timeIntervalSince(last) < Self.overlap {
                destination = .none
            }
            if context.panelOpen { heldByPanel[ask.id] = alert }
            let t = Told(alert: alert, destination: destination)
            told[ask.id] = t
            out.append(t)
        }
        return out
    }

    /// The panel closed. The questions it was holding that are still open
    /// are told now, where any alert would go (Mo, else a notification);
    /// each one only this once. Questions answered meanwhile are dropped.
    public mutating func panelClosed(stillOpen asks: [Ask], in context: AlertContext, now: Date) -> [Told] {
        let held = heldByPanel
        heldByPanel = [:]
        return asks.compactMap { ask in
            guard var alert = held[ask.id] else { return nil }
            alert.at = now
            let t = Told(alert: alert, destination: AlertRouter.destination(for: alert, in: context))
            told[ask.id] = t
            return t
        }
    }

    /// The questions that went: what to take back (a notification, Mo's
    /// message). Each is taken back once.
    public mutating func gone(_ ids: [String]) -> [Told] {
        ids.compactMap { id in
            heldByPanel[id] = nil
            // Forgotten once taken back: a long-running app doesn't keep every
            // question it ever saw (a question never comes back with its id).
            guard let t = told.removeValue(forKey: id) else { return nil }
            return t.destination == .none ? nil : t
        }
    }

    /// How many questions are remembered (tests: it stays small).
    var remembered: Int { told.count }

    /// An event alert is about to be routed. False when a question already
    /// told you about the same session just now: skip the event's alert.
    /// True otherwise, and it is remembered so a question right after it
    /// doesn't post a second banner.
    public mutating func allows(_ alert: SessionAlert, now: Date) -> Bool {
        guard alert.kind == .needsYou, alert.reason == "question" || alert.reason == "permission" else { return true }
        let covered = told.values.contains {
            $0.alert.slotId == alert.slotId && $0.destination != .none && now.timeIntervalSince($0.alert.at) < Self.overlap
        }
        if covered { return false }
        eventAlerted[alert.slotId] = now
        eventAlerted = eventAlerted.filter { now.timeIntervalSince($0.value) < Self.overlap }
        return true
    }
}
