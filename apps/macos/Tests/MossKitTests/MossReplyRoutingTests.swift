import Foundation
import Testing
@testable import MossKit

private func reply(_ id: String, to: String?) -> MossMessage {
    MossMessage(id: id, from: .moss, text: "ok", replyTo: to)
}

@Suite struct MossReplyRoutingTests {
    @Test func onTheMossPageNothing() {
        let ctx = AlertContext(petEnabled: true, petVisible: true, panelOpen: true)
        #expect(MossReplyRouter.destination(in: ctx, onMossPage: true) == .none)
    }

    @Test func panelOnAnotherPageWithMoInViewIsMo() {
        let ctx = AlertContext(petEnabled: true, petVisible: true, panelOpen: true)
        #expect(MossReplyRouter.destination(in: ctx, onMossPage: false) == .pet)
    }

    @Test func moInViewIsMo() {
        let ctx = AlertContext(petEnabled: true, petVisible: true)
        #expect(MossReplyRouter.destination(in: ctx, onMossPage: false) == .pet)
        #expect(MossReplyRouter.destination(in: ctx, onMossPage: true) == .pet, "the panel is closed")
    }

    @Test func otherwiseANotification() {
        #expect(MossReplyRouter.destination(in: AlertContext(), onMossPage: false) == .notification)
        let hidden = AlertContext(petEnabled: true, petVisible: false, panelOpen: true)
        #expect(MossReplyRouter.destination(in: hidden, onMossPage: false) == .notification)
    }

    @Test func quietHoursAndTheSettingStillCount() {
        #expect(MossReplyRouter.destination(in: AlertContext(doNotDisturb: true), onMossPage: false) == .none)
        let onlyNotes = AlertContext(mode: .notificationsOnly, petEnabled: true, petVisible: true)
        #expect(MossReplyRouter.destination(in: onlyNotes, onMossPage: false) == .notification)
    }

    @Test func historyAtLaunchIsNeverTold() {
        var watch = MossReplyWatch()
        watch.sent("1")
        #expect(watch.arrived([reply("2", to: "1")]).isEmpty)
    }

    @Test func aNewAnswerToThePanelIsToldOnce() {
        var watch = MossReplyWatch()
        _ = watch.arrived([])
        watch.sent("1")
        let new = [reply("2", to: "1")]
        #expect(watch.arrived(new).map(\.id) == ["2"])
        #expect(watch.arrived(new).isEmpty)
    }

    @Test func answersToSomethingElseAreNotTold() {
        var watch = MossReplyWatch()
        _ = watch.arrived([])
        #expect(watch.arrived([reply("2", to: "from-the-terminal"), reply("3", to: nil)]).isEmpty)
        let digest = MossMessage(id: "4", from: .events, text: "[动态]")
        #expect(watch.arrived([digest]).isEmpty)
    }

    @Test func theNoteOpensTheMossPage() {
        let note = MossReplyRouter.note(for: MossMessage(id: "9", from: .moss, text: "**api-review** is waiting\nfor you"), lang: .en)
        #expect(note.slotId == MossReplyRouter.slot)
        #expect(note.title == "Moss replied")
        #expect(note.body == "**api-review** is waiting for you")
    }

    @Test func aLongAnswerIsCut() {
        let long = MossMessage(id: "9", from: .moss, text: String(repeating: "a", count: 200))
        #expect(MossReplyRouter.excerpt(long).count == 90)
    }
}
