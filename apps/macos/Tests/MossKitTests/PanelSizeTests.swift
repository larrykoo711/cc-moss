import Foundation
import Testing
@testable import MossKit

@Suite struct PanelSizeTests {
    @Test func threeSizesByCount() {
        #expect(PanelSize.tier(count: 0, previous: nil) == .compact)
        #expect(PanelSize.tier(count: 4, previous: nil) == .compact)
        #expect(PanelSize.tier(count: 5, previous: nil) == .regular)
        #expect(PanelSize.tier(count: 9, previous: nil) == .regular)
        #expect(PanelSize.tier(count: 10, previous: nil) == .tall)
    }

    @Test func upAtOnce() {
        #expect(PanelSize.tier(count: 5, previous: .compact) == .regular)
        #expect(PanelSize.tier(count: 12, previous: .compact) == .tall)
    }

    @Test func downOnlyTwoBelow() {
        #expect(PanelSize.tier(count: 4, previous: .regular) == .regular, "4 ↔ 5 doesn't flap")
        #expect(PanelSize.tier(count: 3, previous: .regular) == .compact)
        #expect(PanelSize.tier(count: 9, previous: .tall) == .tall)
        #expect(PanelSize.tier(count: 8, previous: .tall) == .regular)
        #expect(PanelSize.tier(count: 2, previous: .tall) == .compact)
    }

    @Test func fixedSizes() {
        #expect(PanelSize.height(.compact, visible: 1000) == 460)
        #expect(PanelSize.height(.regular, visible: 1000) == 580)
    }

    @Test func tallIsSeventyPercentWithinLimits() {
        #expect(PanelSize.height(.tall, visible: 1100) == 770, "70%")
        #expect(PanelSize.height(.tall, visible: 2000) == 900, "at most 900 on a big screen")
        #expect(PanelSize.height(.tall, visible: 900) == 720, "at least 720")
    }

    @Test func aSmallScreenCaps() {
        #expect(PanelSize.height(.tall, visible: 700) == 620, "screen less 80")
        #expect(PanelSize.height(.regular, visible: 600) == 520)
        #expect(PanelSize.height(.compact, visible: 450) == 420, "never below 420")
    }
}
