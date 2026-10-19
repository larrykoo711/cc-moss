import Foundation

/// How tall the panel is (user 2026-10-19): three sizes by how many
/// sessions you have, decided when the panel opens, never while it's open.
public enum PanelTier: Int, Comparable, Sendable {
    case compact, regular, tall

    public static func < (a: PanelTier, b: PanelTier) -> Bool { a.rawValue < b.rawValue }
}

public enum PanelSize {
    /// Session counts from which the panel grows.
    static func threshold(_ tier: PanelTier) -> Int {
        switch tier {
        case .compact: 0
        case .regular: 5
        case .tall: 10
        }
    }

    /// The size for `count` of your sessions (not Moss, not closed). Up as
    /// soon as the count reaches a bigger size; down only once it's 2 below
    /// the current size's threshold, so 4 ↔ 5 doesn't resize every time.
    public static func tier(count: Int, previous: PanelTier?) -> PanelTier {
        let raw: PanelTier = count >= threshold(.tall) ? .tall : count >= threshold(.regular) ? .regular : .compact
        guard let previous, raw < previous else { return raw }
        return count <= threshold(previous) - 2 ? raw : previous
    }

    public static let compact: Double = 460
    public static let regular: Double = 580
    public static let lowest: Double = 420

    /// The height in points on a screen with `visible` points of height to
    /// spare: the tall size is 70% of it, between 720 and 900; no size is
    /// taller than the screen less 80, or shorter than 420.
    public static func height(_ tier: PanelTier, visible: Double) -> Double {
        let wanted: Double = switch tier {
        case .compact: compact
        case .regular: regular
        case .tall: min(900, max(720, visible * 0.7))
        }
        return max(lowest, min(wanted, visible - 80))
    }
}
