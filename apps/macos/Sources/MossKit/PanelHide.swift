import Foundation

/// When the sessions panel goes away by itself. A click elsewhere, Esc and
/// a toggle always close it; losing the keyboard closes it only when
/// something actually took the keyboard, never while the app was simply
/// not active (a link that launched the app: AppKit takes the key away
/// with nobody receiving it).
public enum PanelHide {
    public enum Reason: Equatable, Sendable {
        case clickOutside
        case escape
        case toggle
        /// The panel lost the keyboard. `keyTaken`: another window became key
        /// (ours, or the app is active so the keyboard went somewhere real).
        case resignedKey(keyTaken: Bool)
        /// The app stopped being the active one, after it had been.
        case appResignedActive
    }

    /// - appActive: the app is active right now.
    /// - justOpenedByLink: within the short hold after a link opened it.
    public static func shouldHide(_ reason: Reason, appActive: Bool, justOpenedByLink: Bool) -> Bool {
        switch reason {
        case .clickOutside, .escape, .toggle:
            return true
        case let .resignedKey(keyTaken):
            if justOpenedByLink { return false }
            // Key went nowhere while the app isn't active: nothing took it.
            if !keyTaken && !appActive { return false }
            return true
        case .appResignedActive:
            return !justOpenedByLink
        }
    }
}
