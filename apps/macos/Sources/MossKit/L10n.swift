import Foundation

/// The interface language. Mirrors the CLI's `tr!`: every message is written
/// in both languages side by side, and one is picked at run time.
public enum Lang: Equatable, Sendable {
    case en, zh

    /// A tag such as `zh-Hans-CN`, `zh_CN.UTF-8` or `en`. Empty and `auto` say nothing.
    static func parse(_ tag: String?) -> Lang? {
        guard let tag = tag?.trimmingCharacters(in: .whitespaces).lowercased(),
              !tag.isEmpty, tag != "auto" else { return nil }
        return tag.hasPrefix("zh") ? .zh : .en
    }

    /// The order the CLI uses: `CC_MOSS_LANG`, then `[ui] language` in the
    /// config file, then the user's first preferred language.
    public static func detect(
        env: [String: String] = ProcessInfo.processInfo.environment,
        config: String? = nil,
        preferred: [String] = Locale.preferredLanguages
    ) -> Lang {
        parse(env["CC_MOSS_LANG"]) ?? LangChoice(config: config).lang ?? parse(preferred.first) ?? .en
    }
}

/// The choice in Settings › General, stored as the CLI's `[ui] language`.
public enum LangChoice: String, CaseIterable, Sendable {
    case auto, zh, en

    /// Anything but `zh` or `en` follows the system, as in the CLI.
    public init(config: String?) {
        self = config.flatMap { LangChoice(rawValue: $0.trimmingCharacters(in: .whitespaces).lowercased()) } ?? .auto
    }

    public var configValue: String { rawValue }

    /// Nil: follow the system.
    public var lang: Lang? {
        switch self {
        case .auto: nil
        case .zh: .zh
        case .en: .en
        }
    }
}

public enum L {
    /// Decided once at launch; tests set it directly.
    nonisolated(unsafe) public static var lang: Lang = .detect()

    /// The current time; screenshots pin it so demo data reads the same every time.
    nonisolated(unsafe) public static var clock: () -> Date = Date.init

    /// Pick the wording for the current language.
    public static func t(_ en: String, _ zh: String) -> String {
        lang == .zh ? zh : en
    }

    /// `just now`, `5 minutes ago`, `yesterday`, `Oct 3`: how long ago `date` was.
    public static func ago(_ date: Date, now: Date = L.clock(), calendar: Calendar = .current) -> String {
        let s = max(0, Int(now.timeIntervalSince(date)))
        switch s {
        case 0..<60:
            return t("just now", "刚刚")
        case 60..<3600:
            let n = s / 60
            return t("\(n) minute\(n == 1 ? "" : "s") ago", "\(n) 分钟前")
        case 3600..<86_400 where calendar.isDate(date, inSameDayAs: now):
            let n = s / 3600
            return t("\(n) hour\(n == 1 ? "" : "s") ago", "\(n) 小时前")
        default:
            if let yesterday = calendar.date(byAdding: .day, value: -1, to: now),
               calendar.isDate(date, inSameDayAs: yesterday) {
                return t("yesterday", "昨天")
            }
            if s < 3600 * 24 {
                let n = s / 3600
                return t("\(n) hour\(n == 1 ? "" : "s") ago", "\(n) 小时前")
            }
            let f = DateFormatter()
            f.calendar = calendar
            f.timeZone = calendar.timeZone
            f.locale = Locale(identifier: lang == .zh ? "zh_CN" : "en_US")
            f.setLocalizedDateFormatFromTemplate("MMMd")
            return f.string(from: date)
        }
    }
}
