import CoreGraphics
import Foundation
import Testing
@testable import MossKit

@Suite struct PetPlacementTests {
    // Main: 1440 x 900 with a 25 pt menu bar and a 70 pt Dock at the bottom.
    private let main = PetScreen(id: "main", frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
                                 visible: CGRect(x: 0, y: 70, width: 1440, height: 805), isMain: true)
    // A second display to the right.
    private let side = PetScreen(id: "side", frame: CGRect(x: 1440, y: 0, width: 1920, height: 1080),
                                 visible: CGRect(x: 1440, y: 0, width: 1920, height: 1055), isMain: false)
    private let size = CGSize(width: 100, height: 100)

    @Test func clampsIntoVisibleFrame() {
        let visible = main.visible
        let out = PetPlacement.clamp(CGRect(x: -40, y: 20, width: 100, height: 100), into: visible)
        #expect(out.origin == CGPoint(x: 0, y: 70))
        let high = PetPlacement.clamp(CGRect(x: 1400, y: 880, width: 100, height: 100), into: visible)
        #expect(high.origin == CGPoint(x: 1340, y: 775))
        let inside = CGRect(x: 300, y: 300, width: 100, height: 100)
        #expect(PetPlacement.clamp(inside, into: visible) == inside)
    }

    @Test func snapsToEdgeWithin24() {
        let visible = main.visible
        let near = PetPlacement.snap(CGRect(x: 1320, y: 90, width: 100, height: 100), in: visible)
        #expect(near.origin == CGPoint(x: 1340, y: 70))
        let far = CGRect(x: 1315, y: 95, width: 100, height: 100)
        #expect(PetPlacement.snap(far, in: visible) == far)
        let left = PetPlacement.snap(CGRect(x: 24, y: 400, width: 100, height: 100), in: visible)
        #expect(left.origin.x == 0)
    }

    @Test func remembersPerScreen() {
        var placement = PetPlacement()
        placement.remember(CGRect(x: 200, y: 300, width: 100, height: 100), on: main)
        placement.remember(CGRect(x: 1600, y: 500, width: 100, height: 100), on: side)
        #expect(placement.lastScreen == "side")
        #expect(placement.frame(size: size, screens: [main, side]).origin == CGPoint(x: 1600, y: 500))
        // Back on the main screen: its own spot, not the side one's.
        placement.lastScreen = "main"
        #expect(placement.frame(size: size, screens: [main, side]).origin == CGPoint(x: 200, y: 300))
    }

    @Test func followsAResolutionChange() {
        var placement = PetPlacement()
        placement.remember(CGRect(x: 1340, y: 70, width: 100, height: 100), on: main) // bottom right
        let smaller = PetScreen(id: "main", frame: CGRect(x: 0, y: 0, width: 1280, height: 800),
                                visible: CGRect(x: 0, y: 70, width: 1280, height: 705), isMain: true)
        let f = placement.frame(size: size, screens: [smaller])
        #expect(f.origin == CGPoint(x: 1180, y: 70))
    }

    @Test func fallsBackToMainScreen() {
        var placement = PetPlacement()
        placement.remember(CGRect(x: 1600, y: 500, width: 100, height: 100), on: side)
        let f = placement.frame(size: size, screens: [main]) // the side display was unplugged
        #expect(f.origin == CGPoint(x: 1440 - 24 - 100, y: 70 + 24))
    }

    @Test func defaultsToBottomRightAboveTheDock() {
        let f = PetPlacement().frame(size: size, screens: [side, main])
        #expect(f.origin == CGPoint(x: 1316, y: 94))
    }

    @Test func picksTheScreenMostOfTheFrameIsOn() {
        let f = CGRect(x: 1420, y: 400, width: 100, height: 100)
        #expect(PetPlacement.screen(for: f, among: [main, side])?.id == "side")
    }

    @Test func tucksHalfIntoTheNearerSide() {
        let visible = main.visible
        let right = PetPlacement.tucked(CGRect(x: 1200, y: 300, width: 100, height: 100), in: visible)
        #expect(right.midX == visible.maxX)
        let left = PetPlacement.tucked(CGRect(x: 100, y: 300, width: 100, height: 100), in: visible)
        #expect(left.midX == visible.minX)
    }

    @Test func survivesARoundTripThroughDefaults() throws {
        var placement = PetPlacement()
        placement.remember(CGRect(x: 200, y: 300, width: 100, height: 100), on: main)
        let data = try JSONEncoder().encode(placement)
        #expect(try JSONDecoder().decode(PetPlacement.self, from: data) == placement)
    }
}
