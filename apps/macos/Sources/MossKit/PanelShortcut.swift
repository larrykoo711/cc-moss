import Foundation

/// A modifier key that can be tapped twice to call the panel.
public enum TapKey: String, CaseIterable, Sendable {
    case control, option, shift, command

    public var glyph: String {
        switch self {
        case .control: "⌃"
        case .option: "⌥"
        case .shift: "⇧"
        case .command: "⌘"
        }
    }
}

/// How the panel is called from anywhere: one shortcut, recorded like any
/// other. It can be a modifier tapped twice (double Control by default,
/// like Raycast's and Spotlight's double-tap options) or a key combination.
public enum PanelShortcut: Equatable, Sendable {
    case doubleTap(TapKey)
    case combo(HotKey)
    case none

    public static let `default` = PanelShortcut.doubleTap(.control)

    static let key = "panelShortcut"

    public static func load(_ defaults: UserDefaults) -> PanelShortcut {
        guard let raw = defaults.string(forKey: key) else { return .default }
        if raw == "combo" { return HotKey.load(defaults).map(PanelShortcut.combo) ?? .none }
        if raw == "none" { return .none }
        if raw.hasPrefix("double-"), let k = TapKey(rawValue: String(raw.dropFirst("double-".count))) { return .doubleTap(k) }
        return .default
    }

    public static func save(_ s: PanelShortcut, _ defaults: UserDefaults) {
        switch s {
        case let .doubleTap(k): defaults.set("double-\(k.rawValue)", forKey: key)
        case let .combo(k):
            defaults.set("combo", forKey: key)
            HotKey.save(k, defaults)
        case .none: defaults.set("none", forKey: key)
        }
    }

    /// Keys only: `⌃ ⌃`, `⌃⌥M`.
    public var display: String {
        switch self {
        case let .doubleTap(k): "\(k.glyph) \(k.glyph)"
        case let .combo(k): k.display
        case .none: L.t("None", "未设置")
        }
    }

    /// In a sentence: "tap ⌃ twice", "⌃⌥M".
    public var spoken: String {
        switch self {
        case let .doubleTap(k): L.t("tap \(k.glyph) twice", "连按两下 \(k.glyph)")
        case let .combo(k): k.display
        case .none: L.t("no shortcut", "没有快捷键")
        }
    }

    public var tapKey: TapKey? {
        if case let .doubleTap(k) = self { return k }
        return nil
    }
}

/// Spots a modifier tapped twice: two short presses of it alone, the
/// second within `window` of the first. Any other key or modifier in
/// between, or holding it, starts over.
public struct DoubleTap: Sendable {
    public var window: TimeInterval = 0.35
    /// Longer than this is holding, not tapping.
    public var tapLength: TimeInterval = 0.3

    private var downAt: TimeInterval?
    private var firstTapAt: TimeInterval?

    public init() {}

    /// A modifier change: `key` is the watched modifier down, `others` any other modifier down.
    /// True when this completes a double tap.
    public mutating func modifiers(key control: Bool, others: Bool, at t: TimeInterval) -> Bool {
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
