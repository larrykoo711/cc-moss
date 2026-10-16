import Foundation
import Testing
@testable import MossKit

@Suite struct AlertRouterTests {
    private func alert(_ id: Int = 1, _ kind: AlertKind = .needsYou) -> SessionAlert {
        SessionAlert(id: id, slotId: "A", kind: kind, name: "job-a", at: Date(timeIntervalSince1970: 0))
    }

    private let petOnScreen = AlertContext(mode: .auto, petEnabled: true, petVisible: true)

    @Test func autoPrefersPetWhenVisible() {
        #expect(AlertRouter.destination(for: alert(), in: petOnScreen) == .pet)
    }

    @Test func petVisibleTakesIt() {
        // Every kind, through the stateful router: Mo has it, so no system notification.
        var router = AlertRouter()
        for (i, kind) in AlertKind.allCases.enumerated() {
            #expect(router.route(alert(i, kind), in: petOnScreen) == .pet)
        }
    }

    @Test func petHiddenFallsBackToNotification() {
        let tucked = AlertContext(mode: .auto, petEnabled: true, petVisible: false)
        let off = AlertContext(mode: .auto, petEnabled: false, petVisible: true)
        #expect(AlertRouter.destination(for: alert(), in: tucked) == .notification)
        #expect(AlertRouter.destination(for: alert(), in: off) == .notification)
    }

    @Test func notificationOnly() {
        let ctx = AlertContext(mode: .notificationsOnly, petEnabled: true, petVisible: true)
        #expect(AlertRouter.destination(for: alert(), in: ctx) == .notification)
    }

    @Test func petOnly() {
        let hidden = AlertContext(mode: .petOnly, petEnabled: true, petVisible: false)
        let off = AlertContext(mode: .petOnly, petEnabled: false, petVisible: false)
        #expect(AlertRouter.destination(for: alert(), in: hidden) == .pet)
        // Mo switched off can't take it: Notification Center does, so nothing is lost.
        #expect(AlertRouter.destination(for: alert(), in: off) == .notification)
    }

    @Test func panelOpenSuppresses() {
        for mode in AlertMode.allCases {
            let ctx = AlertContext(mode: mode, petEnabled: true, petVisible: true, panelOpen: true)
            #expect(AlertRouter.destination(for: alert(), in: ctx) == .none)
        }
    }

    @Test func dndSuppresses() {
        for mode in AlertMode.allCases {
            let ctx = AlertContext(mode: mode, petEnabled: true, petVisible: false, doNotDisturb: true)
            #expect(AlertRouter.destination(for: alert(), in: ctx) == .none)
        }
    }

    @Test func neverDeliveredTwice() {
        var router = AlertRouter()
        let hidden = AlertContext(mode: .auto, petEnabled: true, petVisible: false)
        #expect(router.route(alert(7), in: petOnScreen) == .pet)
        // Mo walks off; the same alert must not now reach Notification Center.
        #expect(router.route(alert(7), in: hidden) == .none)
        // Nor does one held back while the panel was open come out afterwards.
        #expect(router.route(alert(8), in: AlertContext(panelOpen: true)) == .none)
        #expect(router.route(alert(8), in: hidden) == .none)
        // A new alert still goes somewhere.
        #expect(router.route(alert(9), in: hidden) == .notification)
    }

    /// Three modes × Mo visible or not × panel open or closed × do not disturb.
    @Test func fullMatrix() {
        for mode in AlertMode.allCases {
            for visible in [false, true] {
                for panel in [false, true] {
                    for dnd in [false, true] {
                        let ctx = AlertContext(mode: mode, petEnabled: true, petVisible: visible, panelOpen: panel, doNotDisturb: dnd)
                        let want: AlertDestination = if panel || dnd {
                            .none
                        } else {
                            switch mode {
                            case .notificationsOnly: .notification
                            case .petOnly: .pet
                            case .auto: visible ? .pet : .notification
                            }
                        }
                        #expect(AlertRouter.destination(for: alert(), in: ctx) == want, "\(mode) visible=\(visible) panel=\(panel) dnd=\(dnd)")
                    }
                }
            }
        }
    }

    @Test func modeReadsFromDefaults() {
        let defaults = UserDefaults(suiteName: "AlertRouterTests.\(UUID().uuidString)")!
        #expect(AlertMode(defaults: defaults) == .auto)
        defaults.set("pet", forKey: AlertMode.key)
        #expect(AlertMode(defaults: defaults) == .petOnly)
        defaults.set("nonsense", forKey: AlertMode.key)
        #expect(AlertMode(defaults: defaults) == .auto)
    }
}
