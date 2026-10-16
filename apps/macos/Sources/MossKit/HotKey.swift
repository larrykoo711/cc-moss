import Foundation

/// The global shortcut that opens the panel (A20): a key and its modifiers,
/// stored in the user defaults. Registered with the system in the app.
public struct HotKey: Equatable, Sendable {
    public struct Modifiers: OptionSet, Equatable, Sendable {
        public let rawValue: Int
        public init(rawValue: Int) { self.rawValue = rawValue }
        public static let control = Modifiers(rawValue: 1 << 0)
        public static let option = Modifiers(rawValue: 1 << 1)
        public static let shift = Modifiers(rawValue: 1 << 2)
        public static let command = Modifiers(rawValue: 1 << 3)
    }

    /// The hardware key code (`kVK_*`), what the system registers.
    public var keyCode: Int
    /// What the key types, for showing it.
    public var key: String
    public var modifiers: Modifiers

    public init(keyCode: Int, key: String, modifiers: Modifiers) {
        self.keyCode = keyCode
        self.key = key
        self.modifiers = modifiers
    }

    /// ⌃⌥M: M for Moss. ⌥⌘M minimizes all windows in many apps, and
    /// ⌃Space usually switches the input method.
    public static let `default` = HotKey(keyCode: 46, key: "m", modifiers: [.control, .option])

    /// Shift alone would steal a capital letter from every app.
    public var isValid: Bool {
        !modifiers.intersection([.control, .option, .command]).isEmpty
    }

    /// As menus show it: ⌃⌥⇧⌘ then the key.
    public var display: String {
        var s = ""
        if modifiers.contains(.control) { s += "⌃" }
        if modifiers.contains(.option) { s += "⌥" }
        if modifiers.contains(.shift) { s += "⇧" }
        if modifiers.contains(.command) { s += "⌘" }
        return s + Self.name(of: key)
    }

    private static func name(of key: String) -> String {
        switch key {
        case " ": "Space"
        case "\t": "⇥"
        case "\r": "↩"
        default: key.uppercased()
        }
    }

    // MARK: - Defaults

    static let defaultsKey = "panelHotKey"

    /// Unset: the default. Cleared on purpose: nil.
    public static func load(_ defaults: UserDefaults) -> HotKey? {
        guard let stored = defaults.dictionary(forKey: defaultsKey) else { return .default }
        guard let code = stored["keyCode"] as? Int, let key = stored["key"] as? String,
              let mods = stored["modifiers"] as? Int else { return nil }
        return HotKey(keyCode: code, key: key, modifiers: Modifiers(rawValue: mods))
    }

    public static func save(_ hotKey: HotKey?, _ defaults: UserDefaults) {
        if let k = hotKey {
            defaults.set(["keyCode": k.keyCode, "key": k.key, "modifiers": k.modifiers.rawValue], forKey: defaultsKey)
        } else {
            defaults.set([String: Any](), forKey: defaultsKey) // present but empty: cleared
        }
    }
}
