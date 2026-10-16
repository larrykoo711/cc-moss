import Foundation
import Testing
@testable import MossKit

private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

private func event(_ seq: Int, _ kind: String, slot: String = "A", at: TimeInterval? = 0, reason: String? = nil) -> SessionEvent {
    SessionEvent(seq: seq, slotId: slot, name: "job-a", kind: kind, at: at.map { t0.addingTimeInterval($0) }, reason: reason)
}

@Suite struct NotificationPlanTests {
    // The wording is checked by shape, not by language: other suites switch
    // the global `L.lang` while these run.
    @Test func mapsEachKind() {
        var plan = NotificationPlan()
        let all = AlertToggles(on: Set(AlertKind.allCases))
        let events = [
            event(1, "needs_you", reason: "permission"), event(2, "interrupted"), event(3, "finished"), event(4, "restored"),
            // Not worth an alert.
            event(5, "started"), event(6, "working"), event(7, "renamed"), event(8, "ended"), event(9, "closed"),
        ]
        let alerts = plan.alerts(from: events, toggles: all, now: t0)
        #expect(alerts.map(\.kind) == [.needsYou, .interrupted, .finished, .restored])
        #expect(alerts.map(\.id) == [1, 2, 3, 4])

        let notes = alerts.map { NotificationPlan.note(for: $0, title: "billing") }
        #expect(notes.allSatisfy { $0.title.hasPrefix("billing ") && !$0.body.isEmpty })
        #expect(Set(notes.map(\.title)).count == 4)
        #expect(notes.map(\.action) == [.open, .restore, .open, .open])
        #expect(notes.allSatisfy { $0.slotId == "A" && $0.thread == "A" })
        #expect(Set(notes.map(\.id)).count == 4)
    }

    @Test func needsYouSaysWhy() {
        func body(_ reason: String?) -> String {
            NotificationPlan.note(for: SessionAlert(id: 1, slotId: "A", kind: .needsYou, reason: reason, at: t0), title: "x").body
        }
        #expect(Set([body("question"), body("input"), body("permission")]).count == 3)
        #expect(body(nil) == body("permission"))
    }

    @Test func respectsToggles() {
        // Defaults (A9): needs you, interrupted, restored on; finished off.
        let defaults = AlertToggles()
        #expect(AlertKind.allCases.filter { defaults.isOn($0) } == [.needsYou, .interrupted, .restored])

        var toggles = defaults
        toggles.set(.needsYou, false)
        toggles.set(.finished, true)
        var plan = NotificationPlan()
        let alerts = plan.alerts(from: [event(1, "needs_you"), event(2, "finished"), event(3, "interrupted")], toggles: toggles, now: t0)
        #expect(alerts.map(\.kind) == [.finished, .interrupted])
    }

    @Test func togglesReadFromDefaults() {
        let store = UserDefaults(suiteName: "NotificationPlanTests.\(UUID().uuidString)")!
        #expect(AlertToggles(defaults: store) == AlertToggles())
        store.set(true, forKey: AlertToggles.key(.finished))
        store.set(false, forKey: AlertToggles.key(.restored))
        let read = AlertToggles(defaults: store)
        #expect(read.isOn(.finished))
        #expect(!read.isOn(.restored))
        #expect(read.isOn(.needsYou))
    }

    @Test func mergesWithin30s() {
        var plan = NotificationPlan()
        let on = AlertToggles()
        // Same session, same kind, 29 s apart: one alert.
        let first = plan.alerts(from: [event(1, "needs_you", at: 0), event(2, "needs_you", at: 29)], toggles: on, now: t0.addingTimeInterval(29))
        #expect(first.map(\.id) == [1])
        // Another session, or another kind, is its own alert.
        let others = plan.alerts(from: [event(3, "needs_you", slot: "B", at: 30), event(4, "interrupted", at: 30)],
                                 toggles: on, now: t0.addingTimeInterval(30))
        #expect(others.map(\.id) == [3, 4])
        // 30 s after the first, the window is over.
        let later = plan.alerts(from: [event(5, "needs_you", at: 30)], toggles: on, now: t0.addingTimeInterval(30))
        #expect(later.map(\.id) == [5])
        // Without a time from the CLI, arrival time counts.
        let untimed = plan.alerts(from: [event(6, "needs_you", at: nil)], toggles: on, now: t0.addingTimeInterval(40))
        #expect(untimed.isEmpty)
    }

    @Test func firstLaunchStartsAtCursor() throws {
        var cursor = EventCursor()
        #expect(cursor.since == nil)
        // The first page (`--limit 1`) carries old news: only the cursor moves.
        let first = try EventPage.decode(Data(#"""
        {"schema":1,"events":[{"seq":41,"slot_id":"A","name":"api","kind":"needs_you","detail":{"reason":"permission"},"at":"2026-10-16T08:00:00Z"}],"last_seq":41}
        """#.utf8))
        #expect(cursor.advance(first).isEmpty)
        #expect(cursor.since == 41)
        // After that, what is new comes through, in order, without repeats.
        let next = EventPage(events: [event(43, "interrupted"), event(41, "needs_you"), event(42, "needs_you")], lastSeq: 43)
        #expect(cursor.advance(next).map(\.seq) == [42, 43])
        #expect(cursor.since == 43)
        #expect(cursor.advance(EventPage(events: [], lastSeq: 43)).isEmpty)
    }

    @Test func eventDetailDecodes() throws {
        let page = try EventPage.decode(Data(#"""
        {"schema":1,"events":[
          {"seq":1,"slot_id":"A","name":null,"kind":"needs_you","detail":{"reason":"question","message":"Claude asks"},"at":"2026-10-16T08:00:00Z"},
          {"seq":2,"slot_id":"A","name":"a","kind":"renamed","detail":{"from":null,"to":"a"},"at":"not a time"},
          {"seq":3,"slot_id":"A","kind":"ended","detail":{"reason":7}}
        ],"last_seq":3}
        """#.utf8))
        #expect(page.events.map(\.reason) == ["question", nil, nil])
        #expect(page.events[0].at != nil)
        #expect(page.events[1].at == nil)
    }
}

/// A notification center that does what it's told.
@MainActor private final class FakeNotifier: Notifier {
    var status: NotificationAuth
    var answer: NotificationAuth
    var asks = 0
    var posted: [AlertNote] = []

    init(status: NotificationAuth, answer: NotificationAuth = .allowed) {
        self.status = status
        self.answer = answer
    }

    func authorization() async -> NotificationAuth { status }

    func requestAuthorization() async -> NotificationAuth {
        asks += 1
        status = answer
        return answer
    }

    func post(_ note: AlertNote) async -> Bool {
        posted.append(note)
        return true
    }
}

@Suite @MainActor struct NotificationGateTests {
    private let note = NotificationPlan.note(for: SessionAlert(id: 1, slotId: "A", kind: .needsYou, at: t0), title: "job-a")

    @Test func asksOnFirstNeedThenPosts() async {
        let fake = FakeNotifier(status: .notDetermined)
        let gate = NotificationGate(notifier: fake)
        #expect(fake.asks == 0) // never at launch
        #expect(await gate.deliver(note))
        #expect(fake.asks == 1)
        #expect(fake.posted == [note])
        #expect(gate.auth == .allowed)
    }

    @Test func refusedOnceIsNotAskedAgain() async {
        let fake = FakeNotifier(status: .notDetermined, answer: .notDetermined) // dismissed without choosing
        let gate = NotificationGate(notifier: fake)
        #expect(!(await gate.deliver(note)))
        #expect(!(await gate.deliver(note)))
        #expect(fake.asks == 1)
        #expect(fake.posted.isEmpty)
    }

    @Test func deniedPostsNothingAndSaysSo() async {
        let fake = FakeNotifier(status: .denied)
        let gate = NotificationGate(notifier: fake)
        var seen: [NotificationAuth] = []
        gate.onChange = { seen.append($0) }
        #expect(!(await gate.deliver(note)))
        #expect(fake.asks == 0)
        #expect(fake.posted.isEmpty)
        #expect(seen == [.denied])
    }

    @Test func outsideApplicationsIsUnavailable() async {
        let fake = FakeNotifier(status: .notDetermined, answer: .unavailable)
        let gate = NotificationGate(notifier: fake)
        #expect(!(await gate.deliver(note)))
        #expect(gate.auth == .unavailable)
    }

    @Test func settingsCanAskAhead() async {
        let fake = FakeNotifier(status: .notDetermined)
        let gate = NotificationGate(notifier: fake)
        await gate.requestNow()
        #expect(gate.auth == .allowed)
        #expect(await gate.deliver(note))
        #expect(fake.asks == 1)
    }
}
