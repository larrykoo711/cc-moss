import Foundation
import Testing
@testable import MossKit

@Suite struct PanelPlacementTests {
    // The built-in screen at the origin, an external one to its left (as in
    // the cold-start report).
    let builtIn = PanelPlacement.Screen(frame: CGRect(x: 0, y: 0, width: 1512, height: 982),
                                        visible: CGRect(x: 0, y: 0, width: 1512, height: 944))
    let external = PanelPlacement.Screen(frame: CGRect(x: -2560, y: 0, width: 2560, height: 1440),
                                         visible: CGRect(x: -2560, y: 0, width: 2560, height: 1415))
    let size = CGSize(width: 476, height: 460)

    @Test func underTheIconWhenItIsLaidOut() {
        // The icon in the menu bar: left edges lined up, just under the bar.
        let icon = CGRect(x: 800, y: 952, width: 24, height: 24)
        let f = PanelPlacement.frame(anchor: icon, screens: [builtIn, external], mouse: .zero, size: size)
        #expect(f.minX == 800)
        #expect(f.maxY == builtIn.visible.maxY)
    }

    @Test func anEmptyIconFrameFallsBackToTheTopRightUnderThePointer() {
        let f = PanelPlacement.frame(anchor: .zero, screens: [builtIn, external],
                                     mouse: CGPoint(x: -1000, y: 700), size: size)
        #expect(external.visible.contains(f))
        #expect(f.maxX == external.visible.maxX - 8)
        #expect(f.maxY == external.visible.maxY - 8)
    }

    @Test func noIconAndNoScreenUnderThePointerUsesTheMainScreen() {
        let f = PanelPlacement.frame(anchor: nil, screens: [builtIn, external], mouse: CGPoint(x: 9999, y: 9999), size: size)
        #expect(builtIn.visible.contains(f))
    }

    /// What the cold start gave: a small rectangle at the bottom left, not
    /// in any menu bar.
    @Test func anIconNotInAMenuBarIsNotUsed() {
        let f = PanelPlacement.frame(anchor: CGRect(x: 8, y: 0, width: 24, height: 22), screens: [builtIn],
                                     mouse: CGPoint(x: 100, y: 100), size: size)
        #expect(f.maxY == builtIn.visible.maxY - 8, "the top right, not the bottom left")
        #expect(f.maxX == builtIn.visible.maxX - 8)
    }

    @Test func anIconOffEveryScreenIsNotUsed() {
        let f = PanelPlacement.frame(anchor: CGRect(x: 8, y: -500, width: 24, height: 24), screens: [builtIn],
                                     mouse: CGPoint(x: 100, y: 100), size: size)
        #expect(builtIn.visible.contains(f), "the cold-start frame below every screen never happens again")
    }

    @Test func aPanelThatWouldSpillIsKeptInside() {
        // An icon near the right edge: the panel slides left to fit.
        let icon = CGRect(x: 1500, y: 955, width: 10, height: 24)
        let f = PanelPlacement.frame(anchor: icon, screens: [builtIn], mouse: .zero, size: size)
        #expect(f.maxX <= builtIn.visible.maxX - 8)
        #expect(builtIn.visible.contains(f))
    }
}
