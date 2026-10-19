import Foundation
import Testing
@testable import MossKit

@Suite struct AskAlertRoutingTests {
    private let now = Date(timeIntervalSince1970: 1_000_000)
    private let petOnScreen = AlertContext(mode: .auto, petEnabled: true, petVisible: true)
    private let noPet = AlertContext(mode: .auto)

    private func arrive(_ alerts: inout AskAlerts, _ list: [Ask], _ ctx: AlertContext, at t: Date? = nil) -> [AskAlerts.Told] {
        alerts.arrived(list, slot: { _ in "01A" }, title: { $0.sessionName ?? "?" }, in: ctx, now: t ?? now)
    }

    @Test func aQuestionGoesToMoWhenMoIsInView() {
        var alerts = AskAlerts()
        let told = arrive(&alerts, [ask()], petOnScreen)
        #expect(told.map(\.destination) == [.pet])
        #expect(told[0].alert.askId == "a1b2c3d4e5f6a7b8")
        #expect(told[0].alert.kind == .needsYou)
        #expect(told[0].alert.reason == "question")
        #expect(told[0].alert.slotId == "01A")
    }

    @Test func otherwiseANotification() {
        var alerts = AskAlerts()
        let told = arrive(&alerts, [ask(askJSON(kind: "permission"))], noPet)
        #expect(told.map(\.destination) == [.notification])
        #expect(told[0].alert.reason == "permission")
    }

    @Test func nowhereWhileThePanelIsOpen() {
        var alerts = AskAlerts()
        #expect(arrive(&alerts, [ask()], AlertContext(mode: .auto, panelOpen: true)).map(\.destination) == [AlertDestination.none])
    }

    @Test func aQuestionTheOpenPanelHeldGoesToMoWhenThePanelCloses() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask()], AlertContext(mode: .auto, petEnabled: true, petVisible: true, panelOpen: true))
        let told = alerts.panelClosed(stillOpen: [ask()], in: petOnScreen, now: now)
        #expect(told.map(\.destination) == [.pet])
        #expect(told[0].alert.askId == "a1b2c3d4e5f6a7b8")
        // Told now, so taken back when it goes; and never told a third time.
        #expect(alerts.panelClosed(stillOpen: [ask()], in: petOnScreen, now: now).isEmpty)
        #expect(alerts.gone(["a1b2c3d4e5f6a7b8"]).map(\.destination) == [.pet])
    }

    @Test func aQuestionAnsweredInThePanelIsNotToldAfterwards() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask()], AlertContext(mode: .auto, panelOpen: true))
        #expect(alerts.panelClosed(stillOpen: [], in: petOnScreen, now: now).isEmpty)
        #expect(alerts.panelClosed(stillOpen: [ask()], in: petOnScreen, now: now).isEmpty)
    }

    @Test func aQuestionAlreadyToldElsewhereIsNotToldAgainOnClose() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask()], petOnScreen)
        #expect(alerts.panelClosed(stillOpen: [ask()], in: petOnScreen, now: now).isEmpty)
    }

    @Test func quietHoursStillHoldWhenThePanelCloses() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask()], AlertContext(mode: .auto, panelOpen: true))
        let quiet = AlertContext(mode: .auto, petEnabled: true, petVisible: true, doNotDisturb: true)
        #expect(alerts.panelClosed(stillOpen: [ask()], in: quiet, now: now).map(\.destination) == [AlertDestination.none])
    }

    @Test func theSameQuestionIsToldOnce() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask()], noPet)
        #expect(arrive(&alerts, [ask()], petOnScreen).isEmpty)
    }

    @Test func goneTakesBackWhatWasSaid() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask(askJSON(id: "q1")), ask(askJSON(id: "q2"))], noPet)
        let back = alerts.gone(["q1"])
        #expect(back.map(\.alert.askId) == ["q1"])
        #expect(back.map(\.destination) == [.notification])
        #expect(alerts.gone(["q1"]).isEmpty) // once
        #expect(alerts.gone(["never-seen"]).isEmpty)
    }

    @Test func nothingToTakeBackFromNowhere() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask()], AlertContext(mode: .auto, panelOpen: true))
        #expect(alerts.gone(["a1b2c3d4e5f6a7b8"]).isEmpty)
    }

    @Test func theEventForTheSameQuestionIsSkipped() {
        var alerts = AskAlerts()
        _ = arrive(&alerts, [ask()], noPet)
        let event = SessionAlert(id: 7, slotId: "01A", kind: .needsYou, reason: "question", at: now)
        let allowed1 = alerts.allows(event, now: now.addingTimeInterval(5))
        #expect(!allowed1)
        // Another session, or long after: its own alert.
        let allowed2 = alerts.allows(SessionAlert(id: 8, slotId: "02B", kind: .needsYou, reason: "question", at: now), now: now)
        #expect(allowed2)
        let allowed3 = alerts.allows(event, now: now.addingTimeInterval(120))
        #expect(allowed3)
        // Waiting for input is not a question.
        let allowed4 = alerts.allows(SessionAlert(id: 9, slotId: "01A", kind: .needsYou, reason: "input", at: now), now: now)
        #expect(allowed4)
    }

    @Test func aQuestionRightAfterItsEventPostsNoSecondBanner() {
        var alerts = AskAlerts()
        let event = SessionAlert(id: 7, slotId: "01A", kind: .needsYou, reason: "permission", at: now)
        let allowed5 = alerts.allows(event, now: now)
        #expect(allowed5)
        #expect(arrive(&alerts, [ask()], noPet, at: now.addingTimeInterval(3)).map(\.destination) == [AlertDestination.none])
        // Mo folds it into what it holds for that session, with the options.
        var other = AskAlerts()
        let allowed6 = other.allows(event, now: now)
        #expect(allowed6)
        #expect(arrive(&other, [ask()], petOnScreen, at: now.addingTimeInterval(3)).map(\.destination) == [.pet])
    }

    @Test func theNotificationOpensTheQuestion() {
        let alert = SessionAlert(id: 1, slotId: "01A", kind: .needsYou, reason: "question", at: now, askId: "q1")
        let note = NotificationPlan.askNote(for: alert, title: "job-a")
        #expect(note.askId == "q1")
        #expect(note.id == NotificationPlan.askNoteID("q1"))
        #expect(note.title.contains("job-a"))
    }
}
