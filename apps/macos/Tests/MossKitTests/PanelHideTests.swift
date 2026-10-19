import Foundation
import Testing
@testable import MossKit

@Suite struct PanelHideTests {
    @Test func aClickOutsideEscOrToggleAlwaysCloses() {
        for reason: PanelHide.Reason in [.clickOutside, .escape, .toggle] {
            #expect(PanelHide.shouldHide(reason, appActive: false, justOpenedByLink: true))
            #expect(PanelHide.shouldHide(reason, appActive: true, justOpenedByLink: false))
        }
    }

    /// The cold-start link: the app never became active, AppKit takes the key
    /// from the panel and gives it to nobody.
    @Test func losingTheKeyToNobodyWhileInactiveKeepsIt() {
        #expect(!PanelHide.shouldHide(.resignedKey(keyTaken: false), appActive: false, justOpenedByLink: false))
    }

    @Test func anotherWindowTakingTheKeyCloses() {
        #expect(PanelHide.shouldHide(.resignedKey(keyTaken: true), appActive: true, justOpenedByLink: false))
        #expect(PanelHide.shouldHide(.resignedKey(keyTaken: true), appActive: false, justOpenedByLink: false))
        #expect(PanelHide.shouldHide(.resignedKey(keyTaken: false), appActive: true, justOpenedByLink: false))
    }

    @Test func justOpenedByALinkHoldsThroughFocusChanges() {
        #expect(!PanelHide.shouldHide(.resignedKey(keyTaken: true), appActive: true, justOpenedByLink: true))
        #expect(!PanelHide.shouldHide(.appResignedActive, appActive: false, justOpenedByLink: true))
    }

    @Test func theAppGoingInactiveAfterBeingActiveCloses() {
        #expect(PanelHide.shouldHide(.appResignedActive, appActive: false, justOpenedByLink: false))
    }
}
