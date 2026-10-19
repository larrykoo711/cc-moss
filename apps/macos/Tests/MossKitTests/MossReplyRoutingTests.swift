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

    private func finished(_ id: Int, _ slot: String) -> SessionAlert {
        SessionAlert(id: id, slotId: slot, kind: .finished, name: slot, at: Date(timeIntervalSince1970: 0))
    }

    private func mossAlert(_ id: Int) -> SessionAlert {
        MossReplyRouter.alert(for: reply("r\(id)", to: "1"), id: id, at: Date(timeIntervalSince1970: 0))
    }

    @Test func aReplyNeverMergesWithASessionsTurn() {
        var d = PetDirector(mode: .active, phrases: Catchphrases(seed: 7))
        d.take(finished(1, "A"), at: 0)
        d.take(mossAlert(2), at: 0.5) // within the 1.5 s merge window
        #expect(d.message?.kind == .mossReply)
        #expect(d.message?.slots == [MossReplyRouter.slot])
        d.take(finished(3, "B"), at: 1)
        #expect(d.message?.kind == .mossReply, "a session's turn doesn't take its place")
        #expect(d.message?.slots == [MossReplyRouter.slot])
    }

    @Test func severalRepliesAreOneMessageAboutMoss() {
        var d = PetDirector(mode: .active, phrases: Catchphrases(seed: 7))
        d.take(mossAlert(1), at: 0)
        d.take(mossAlert(2), at: 0.5)
        let message = d.message
        #expect(message?.alerts.count == 1)
        #expect(message?.text(names: ["Moss"]).title == L.t("Moss replied", "Moss 回你了"))
        #expect(message?.text(names: ["Moss"]).primary == L.t("Open Panel", "打开面板"))
        #expect(message?.action == .open)
    }

    @Test func aLongAnswerIsCut() {
        let long = MossMessage(id: "9", from: .moss, text: String(repeating: "a", count: 200))
        #expect(MossReplyRouter.excerpt(long).count == 90)
    }
}
