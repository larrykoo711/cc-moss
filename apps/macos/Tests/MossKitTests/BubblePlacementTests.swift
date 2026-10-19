import CoreGraphics
import Testing
@testable import MossKit

@Suite struct BubblePlacementTests {
    private let screen = CGRect(x: 0, y: 0, width: 1440, height: 900)
    private let size = CGSize(width: 326, height: 180) // a 270 pt box and 28 pt margins
    private let margin: CGFloat = 28

    private func place(head: CGPoint, feet: CGPoint? = nil) -> BubblePlacement {
        BubblePlacement.place(size: size, margin: margin, inset: 34, corner: 14,
                              above: head, below: feet ?? CGPoint(x: head.x, y: head.y - 80), visible: screen)
    }

    @Test func aboveAndLeftByDefault() {
        let p = place(head: CGPoint(x: 1300, y: 200))
        #expect(p.leansRight)
        #expect(!p.tailOnTop)
        // The tip lands on Mo's head; the bubble sits left of it.
        #expect(p.frame.minX + margin + p.tailX == 1300)
        #expect(p.frame.minY + margin == 200)
        #expect(p.frame.maxX - margin < 1300 + 40)
    }

    @Test func flipsRightNearTheLeftEdge() {
        let p = place(head: CGPoint(x: 100, y: 200))
        #expect(!p.leansRight)
        #expect(p.tailX == 34)
        #expect(p.frame.minX + margin >= BubblePlacement.edge)
    }

    @Test func goesBelowNearTheTop() {
        let p = place(head: CGPoint(x: 1300, y: 880), feet: CGPoint(x: 1300, y: 800))
        #expect(p.tailOnTop)
        #expect(p.frame.maxY - margin == 800)
    }

    @Test func staysOnScreenOnTheRight() {
        let p = place(head: CGPoint(x: 1436, y: 200))
        #expect(p.frame.maxX - margin <= screen.maxX - BubblePlacement.edge)
        #expect(p.tailX <= size.width - 2 * margin - 14 - 8)
    }

    // MARK: A question box that changes height (user 2026-10-19: Previous / Next made it jump about)

    private func box(_ height: CGFloat, head: CGPoint, feet: CGPoint, keep: BubblePlacement.Side?) -> BubblePlacement {
        BubblePlacement.place(size: CGSize(width: 420, height: height), margin: margin, inset: 34, corner: 18,
                              above: head, below: feet, visible: screen, keep: keep)
    }

    @Test func aShorterStepStaysOnTheSideItWasOn() {
        // Mo high on the screen: the tall first step had to go below.
        let head = CGPoint(x: 1300, y: 640), feet = CGPoint(x: 1300, y: 560)
        let tall = box(560, head: head, feet: feet, keep: nil)
        #expect(tall.tailOnTop)
        // The next step is short enough for above, but it stays below.
        let short = box(300, head: head, feet: feet, keep: tall.side)
        #expect(short.tailOnTop)
        #expect(short.frame.maxY - margin == 560, "still hanging from Mo's feet")
    }

    @Test func itMovesOnlyWhenItNoLongerFits() {
        let head = CGPoint(x: 1300, y: 300), feet = CGPoint(x: 1300, y: 220)
        let short = box(300, head: head, feet: feet, keep: nil)
        #expect(!short.tailOnTop)
        // Taller than the room above: below fits, so it moves there.
        let tall = box(700, head: head, feet: feet, keep: short.side)
        #expect(!tall.tailOnTop || tall.frame.maxY <= screen.maxY + margin)
        #expect(tall.frame.maxY - margin <= screen.maxY - BubblePlacement.edge)
        #expect(tall.frame.minY + margin >= screen.minY + BubblePlacement.edge)
    }

    @Test func tooTallForEitherSideStaysOnTheScreen() {
        let head = CGPoint(x: 1300, y: 500), feet = CGPoint(x: 1300, y: 420)
        let p = box(900, head: head, feet: feet, keep: nil)
        #expect(p.frame.maxY - margin <= screen.maxY - BubblePlacement.edge)
        #expect(p.frame.minY + margin >= screen.minY + BubblePlacement.edge)
    }
}
