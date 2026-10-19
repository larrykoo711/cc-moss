import Foundation

/// Settings › Moss › Checkup: `moss.daily_checkup` and `moss.daily_brief`
/// in cc-moss's config (T2.4). Both on unless the config says otherwise.
public enum MossDaily {
    public static let checkupKey = "moss.daily_checkup"
    public static let briefKey = "moss.daily_brief"

    /// A config value as a switch: missing, unreadable or anything but a
    /// clear "off" counts as on.
    public static func isOn(_ value: String?) -> Bool {
        guard let v = value?.trimmingCharacters(in: .whitespaces).lowercased() else { return true }
        return !["false", "0", "off", "no"].contains(v)
    }

    /// The value written back.
    public static func value(_ on: Bool) -> String { on ? "true" : "false" }
}
