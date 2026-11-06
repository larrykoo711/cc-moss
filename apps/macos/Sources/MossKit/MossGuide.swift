import Foundation

/// Pointing people to the Moss page (F3): until they open it once, the rail's
/// Moss icon carries a dot while Moss is on. Kept on this Mac only.
public enum MossGuide {
    public static let key = "mossGuideSeen"

    public static func seen(_ defaults: UserDefaults = .standard) -> Bool {
        defaults.bool(forKey: key)
    }

    public static func markSeen(_ defaults: UserDefaults = .standard) {
        defaults.set(true, forKey: key)
    }

    /// Whether the rail points at Moss: it is on, and its page was never opened.
    public static func hints(mossOn: Bool, seen: Bool) -> Bool {
        mossOn && !seen
    }

    /// Whether the Moss page says what the quick bar is for: nothing was ever
    /// sent from it.
    public static func explainsQuickBar(uses: [MossQuickUse]) -> Bool {
        uses.isEmpty
    }
}
