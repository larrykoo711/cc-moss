import Foundation
import Testing
@testable import MossKit

@Suite struct HoverGateTests {
    let here = CGPoint(x: 100, y: 200)

    @Test func hoverCountsWhenNothingScrolled() {
        #expect(HoverGate().allowsHover(pointer: here))
    }

    @Test func rowsSlidingUnderAStillPointerDontCount() {
        let gate = HoverGate()
        gate.scrolled(pointer: here)
        #expect(!gate.allowsHover(pointer: here), "the list moved, the pointer didn't")
        #expect(gate.allowsHover(pointer: CGPoint(x: 101, y: 200)), "it moved: a real hover")
        #expect(gate.allowsHover(pointer: here), "and from then on, until the next scroll")
    }
}
