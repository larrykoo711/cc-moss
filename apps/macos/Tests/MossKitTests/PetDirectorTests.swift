import Foundation
import Testing
@testable import MossKit

@Suite struct PetDirectorTests {
    private func alert(_ id: Int, _ slot: String = "A", _ kind: AlertKind = .needsYou) -> SessionAlert {
        SessionAlert(id: id, slotId: slot, kind: kind, name: "job-\(slot)", at: Date(timeIntervalSince1970: 0))
    }

    private func director(_ mode: PetInteraction) -> PetDirector {
        PetDirector(mode: mode, phrases: Catchphrases(seed: 7))
    }

    private func isPuff(_ d: PetDisplay) -> Bool {
        if case .puff = d { return true }
        return false
    }

    private func isBox(_ d: PetDisplay) -> Bool {
        if case .box = d { return true }
        return false
    }

    private func puffID(_ d: PetDisplay) -> Int? {
        if case let .puff(id, _) = d { return id }
        return nil
    }

    @Test func quietShowsNothingUntilClicked() {
        var d = director(.quiet)
        let hinted = d.take(alert(1), at: 0) // the face still hints
        #expect(hinted)
        #expect(d.display(at: 0.1) == .nothing)
        d.pointer(onMo: true, at: 1)
        #expect(d.display(at: 1.1) == .nothing) // hovering is not enough in quiet mode
        let click = d.click(at: 2)
        #expect(click == .showBox)
        #expect(isBox(d.display(at: 2.1)))
    }

    @Test func everydayShowsPuffsNotTheBox() {
        var d = director(.everyday)
        let hinted = d.take(alert(1), at: 0)
        #expect(hinted)
        let shown = d.display(at: 0.1)
        #expect(isPuff(shown))
        #expect(!isBox(shown))
        if case let .puff(_, text) = shown {
            #expect(Catchphrases.english.contains(text) || Catchphrases.chinese.contains(text))
        }
        // It fades by itself and leaves nothing on screen.
        #expect(d.nextChange(after: 0.1) == PetDirector.puffFor)
        #expect(d.display(at: PetDirector.puffFor + 0.1) == .nothing)
    }

    @Test func hoverExpandsToTheBox() {
        var d = director(.everyday)
        d.take(alert(1), at: 0)
        d.pointer(onMo: true, at: 1)
        #expect(d.display(at: 1.1) == .box(PetMessage(alert(1))))
        // Long after the puff went, hovering still opens what Mo holds.
        var later = director(.everyday)
        later.take(alert(1), at: 0)
        later.pointer(onMo: true, at: 30)
        #expect(isBox(later.display(at: 30.1)))
    }

    @Test func pointerAlreadyOnMoOpensTheBox() {
        var d = director(.everyday)
        d.pointer(onMo: true, at: 0)
        d.take(alert(1), at: 1)
        #expect(isBox(d.display(at: 1.1)))
    }

    @Test func activePopsTheBox() {
        var d = director(.active)
        let hinted = d.take(alert(1), at: 0)
        #expect(hinted)
        #expect(isBox(d.display(at: 0)))
        // Needs-you stays until it's handled.
        #expect(isBox(d.display(at: 600)))
        d.dismiss()
        #expect(d.display(at: 601) == .nothing)
    }

    @Test func mergesWithin1500ms() {
        var d = director(.everyday)
        let hinted = d.take(alert(1, "A"), at: 0)
        #expect(hinted)
        let first = puffID(d.display(at: 0))
        let hinted2 = d.take(alert(2, "B"), at: 1.0) // folded in: no second hint
        #expect(!hinted2)
        #expect(puffID(d.display(at: 1.1)) == first)
        #expect(d.message?.slots == ["A", "B"])
        // After the window, the same kind is a new moment: a new puff, same message.
        let hinted3 = d.take(alert(3, "C"), at: 2.0)
        #expect(hinted3)
        #expect(puffID(d.display(at: 2.1)) != first)
        #expect(d.message?.slots == ["A", "B", "C"])
    }

    @Test func oneBubbleAtATime() {
        var d = director(.active)
        d.take(alert(1, "A", .restored), at: 0)
        // Needs-you outranks back: it takes over.
        let hinted = d.take(alert(2, "B", .needsYou), at: 1)
        #expect(hinted)
        #expect(d.message?.kind == .needsYou)
        // Done ranks below: dropped, the box keeps saying needs-you.
        let hinted2 = d.take(alert(3, "C", .finished), at: 2)
        #expect(!hinted2)
        #expect(d.display(at: 2.1) == .box(PetMessage(alert(2, "B", .needsYou))))
    }

    @Test func noRepeatWithin60s() {
        var d = director(.everyday)
        d.take(alert(1), at: 0)
        d.dismiss()
        let hinted = d.take(alert(2), at: 30)
        #expect(!hinted)
        #expect(d.display(at: 30.1) == .nothing)
        // Still there if you come over.
        d.pointer(onMo: true, at: 31)
        #expect(isBox(d.display(at: 31.1)))
        d.pointer(onMo: false, at: 32)
        d.dismiss()
        let hinted2 = d.take(alert(3), at: 61)
        #expect(hinted2)
        #expect(isPuff(d.display(at: 61.1)))
    }

    @Test func dndKeepsOnlyTheFace() {
        for mode in PetInteraction.allCases {
            var d = director(mode)
            d.doNotDisturb = true
            let hinted = d.take(alert(1), at: 0)
            #expect(!hinted)
            d.pointer(onMo: true, at: 1)
            let click = d.click(at: 2)
            #expect(click == .openPanel)
            #expect(d.display(at: 2.1) == .nothing)
        }
        // Something held when do not disturb starts stays out of sight.
        var d = director(.active)
        d.take(alert(1), at: 0)
        d.doNotDisturb = true
        #expect(d.display(at: 1) == .nothing)
    }

    @Test func boxStaysWhilePointerIsOnIt() {
        var d = director(.everyday)
        d.take(alert(1), at: 0)
        d.pointer(onMo: true, at: 1)
        d.pointer(onMo: false, at: 2)
        // On the way from Mo to the box: still there.
        #expect(isBox(d.display(at: 3)))
        d.pointer(onBubble: true, at: 3)
        #expect(isBox(d.display(at: 60)))
        d.pointer(onBubble: false, at: 60)
        #expect(isBox(d.display(at: 61)))
        #expect(d.nextChange(after: 60) == 60 + PetDirector.collapseAfter)
        #expect(d.display(at: 60 + PetDirector.collapseAfter + 0.01) == .nothing)
        // Still held: back on Mo, it opens again.
        d.pointer(onMo: true, at: 70)
        #expect(isBox(d.display(at: 70.1)))
    }

    @Test func clickOnAnOpenBoxOpensThePanel() {
        var d = director(.active)
        d.take(alert(1), at: 0)
        let click = d.click(at: 1)
        #expect(click == .openPanel)
        var empty = director(.everyday)
        let click2 = empty.click(at: 1)
        #expect(click2 == .openPanel)
    }

    @Test func handledSessionsLeaveTheMessage() {
        var d = director(.active)
        d.take(alert(1, "A"), at: 0)
        d.take(alert(2, "B"), at: 0.5)
        d.keep { $0.slotId != "A" }
        #expect(d.message?.slots == ["B"])
        d.keep { _ in false }
        #expect(d.message == nil)
        #expect(d.display(at: 1) == .nothing)
    }

    @Test func activeBoxesThatAreNotNeedsYouGoByThemselves() {
        var d = director(.active)
        d.take(alert(1, "A", .interrupted), at: 0)
        #expect(isBox(d.display(at: 19)))
        #expect(d.display(at: 20.1) == .nothing)
    }
}
