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
}
