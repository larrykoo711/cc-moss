import Foundation

/// How the panel is called from anywhere: tap Control twice (the default,
/// like Raycast's and Spotlight's double-tap options), a key combination,
/// or nothing.
public enum PanelShortcut: Equatable, Sendable {
    case doubleControl
    case combo(HotKey)
    case none

    static let key = "panelShortcut"

    /// Unset: double Control. A combination keeps its keys in `HotKey`'s own entry.
    public static func load(_ defaults: UserDefaults) -> PanelShortcut {
        switch defaults.string(forKey: key) {
        case "combo": HotKey.load(defaults).map(PanelShortcut.combo) ?? .none
        case "none": .none
        default: .doubleControl
        }
    }

    public static func save(_ s: PanelShortcut, _ defaults: UserDefaults) {
        switch s {
        case .doubleControl: defaults.set("double-control", forKey: key)
        case let .combo(k):
            defaults.set("combo", forKey: key)
            HotKey.save(k, defaults)
        case .none: defaults.set("none", forKey: key)
        }
    }

    public var display: String {
        switch self {
        case .doubleControl: "⌃ ⌃"
        case let .combo(k): k.display
        case .none: L.t("None", "未设置")
        }
    }
}

/// Spots Control tapped twice: two short presses of Control alone, the
/// second within `window` of the first. Any other key or modifier in
/// between, or holding Control, starts over.
public struct DoubleTap: Sendable {
    public var window: TimeInterval = 0.35
    /// Longer than this is holding, not tapping.
    public var tapLength: TimeInterval = 0.3

    private var downAt: TimeInterval?
    private var firstTapAt: TimeInterval?

    public init() {}

    /// A modifier change: `control` is Control down, `others` any other modifier down.
    /// True when this completes a double tap.
    public mutating func modifiers(control: Bool, others: Bool, at t: TimeInterval) -> Bool {
        if others {
            reset()
            return false
        }
        if control {
            downAt = t
            return false
        }
        // Released.
        guard let down = downAt else { return false }
        downAt = nil
        guard t - down <= tapLength else {
            reset()
            return false
        }
        if let first = firstTapAt, t - first <= window {
            firstTapAt = nil
            return true
        }
        firstTapAt = t
        return false
    }

    /// A normal key: whatever it was, it wasn't a double tap.
    public mutating func key() { reset() }

    private mutating func reset() {
        downAt = nil
        firstTapAt = nil
    }
}
