import Foundation

/// Settings › General › Appearance: follow the Mac, or always light or dark.
/// Stored on this Mac only; it covers every window the app shows.
public enum AppearanceChoice: String, CaseIterable, Sendable {
    case system
    case light
    case dark

    public static let key = "appearance"

    /// The stored choice; anything unknown (or nothing) follows the Mac.
    public static func load(_ defaults: UserDefaults = .standard) -> AppearanceChoice {
        defaults.string(forKey: key).flatMap(AppearanceChoice.init(rawValue:)) ?? .system
    }

    public func save(_ defaults: UserDefaults = .standard) {
        if self == .system {
            defaults.removeObject(forKey: Self.key)
        } else {
            defaults.set(rawValue, forKey: Self.key)
        }
    }

    /// The `NSAppearance.Name` raw value to force, or nil to follow the Mac.
    public var appearanceName: String? {
        switch self {
        case .system: nil
        case .light: "NSAppearanceNameAqua"
        case .dark: "NSAppearanceNameDarkAqua"
        }
    }
}
