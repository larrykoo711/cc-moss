import Foundation

/// One version's entry in the site's changelog (`site/src/changelog.json`),
/// the words written for people. The app carries its own copy.
public struct ChangelogEntry: Decodable, Equatable, Sendable {
    /// A line in both languages.
    public struct Words: Decodable, Equatable, Sendable {
        public var zh: String
        public var en: String

        public init(zh: String, en: String) {
            self.zh = zh
            self.en = en
        }

        public func text(_ lang: Lang = L.lang) -> String { lang == .zh ? zh : en }
    }

    public var version: String
    public var date: String
    public var headline: Words
    public var highlights: [Words]
    public var short: Words?

    public init(version: String, date: String, headline: Words, highlights: [Words], short: Words? = nil) {
        self.version = version
        self.date = date
        self.headline = headline
        self.highlights = highlights
        self.short = short
    }
}

/// The changelog bundled into the app at build time (bundle.sh copies it
/// into Resources). Dev runs and tests point at a file with `CC_MOSS_CHANGELOG`.
public struct Changelog: Equatable, Sendable {
    public static let environmentKey = "CC_MOSS_CHANGELOG"

    public var versions: [ChangelogEntry]

    public init(versions: [ChangelogEntry]) {
        self.versions = versions
    }

    public static func decode(_ data: Data) throws -> Changelog {
        struct Wire: Decodable { var versions: [ChangelogEntry] }
        return Changelog(versions: try JSONDecoder().decode(Wire.self, from: data).versions)
    }

    /// Nil when there is no file or it can't be read: no card, nothing else breaks.
    public static func load(_ url: URL?) -> Changelog? {
        guard let url, let data = try? Data(contentsOf: url) else { return nil }
        return try? decode(data)
    }

    /// `CC_MOSS_CHANGELOG`, else the app's own copy.
    public static func location(environment: [String: String] = ProcessInfo.processInfo.environment,
                                bundle: Bundle = .main) -> URL? {
        if let path = environment[environmentKey], !path.isEmpty { return URL(fileURLWithPath: path) }
        return bundle.url(forResource: "changelog", withExtension: "json")
    }

    public func entry(for version: String) -> ChangelogEntry? {
        versions.first { $0.version == version }
    }
}

/// What the top of the panel shows about versions: the card for what's new
/// in the version you just got wins over the bar about a newer one.
public enum PanelTip: Equatable, Sendable {
    case none
    case whatsNew(ChangelogEntry)
    case update(version: String)
}

/// Telling you about versions (F38): a bar in the panel when a newer one is
/// out, a card after an upgrade, and one bubble from Mo per new version.
/// Pure decisions; the app keeps what was seen in `UpdateTipsMemory`.
public enum UpdateTips {
    public static let site = "https://cc-moss.larrykoo.com"
    /// The slot Mo's update message carries: not a session.
    public static let slot = "cc-moss.update"
    /// How many highlights the card shows.
    public static let highlightLimit = 3

    /// `0.3.1` → `v0-3-1`: the site's anchor for that version's card
    /// (`anchorOf` in site/lib/changelog.mjs).
    public static func anchor(_ version: String) -> String {
        "v" + version.replacingOccurrences(of: ".", with: "-")
    }

    /// The site's changelog in your language, at that version's card.
    public static func changelogURL(_ version: String?, lang: Lang = L.lang) -> URL {
        let path = (lang == .zh ? "/zh" : "") + "/changelog/"
        let fragment = version.map { "#" + anchor($0) } ?? ""
        return URL(string: site + path + fragment)!
    }

    /// `a` is a later version than `b`; false when either isn't a version.
    public static func isNewer(_ a: String, than b: String) -> Bool {
        guard let x = VersionMatch.parse(a), let y = VersionMatch.parse(b) else { return false }
        return y.lexicographicallyPrecedes(x)
    }

    /// The version the app counts as running: its own, or (a dev build) the command's.
    static func running(app: String, info: UpdateInfo) -> String {
        VersionMatch.parse(app) != nil ? app : info.current
    }

    /// The newer version worth telling about, if any: checks on, one is
    /// out, and it's later than what runs.
    static func newer(app: String, info: UpdateInfo?, checksOn: Bool) -> String? {
        guard checksOn, let info, info.available else { return nil }
        return isNewer(info.latest, than: running(app: app, info: info)) ? info.latest : nil
    }

    /// The bar's version: a newer one you haven't closed the bar for.
    public static func bar(app: String, info: UpdateInfo?, checksOn: Bool, dismissed: String?) -> String? {
        guard let v = newer(app: app, info: info, checksOn: checksOn), v != dismissed else { return nil }
        return v
    }

    /// The version Mo tells about: a newer one it hasn't told yet.
    public static func bubble(app: String, info: UpdateInfo?, checksOn: Bool, bubbled: String?) -> String? {
        guard let v = newer(app: app, info: info, checksOn: checksOn), v != bubbled else { return nil }
        return v
    }

    /// Mo tells only when an alert would go to Mo right now (in view, not
    /// resting, the panel closed, Mo allowed to tell things). Otherwise it
    /// waits for the next check; the panel's bar says it anyway.
    public static func moTells(in context: AlertContext) -> Bool {
        AlertRouter.destination(for: alert(version: "0", id: 0, at: Date(timeIntervalSince1970: 0)), in: context) == .pet
    }

    /// The alert Mo is handed; the version rides in `name`.
    public static func alert(version: String, id: Int, at: Date) -> SessionAlert {
        SessionAlert(id: id, slotId: slot, kind: .update, name: version, at: at)
    }

    /// The panel opened: the card to show (nil: none) and the version to
    /// remember as seen (nil: leave it). A fresh install shows nothing and
    /// just remembers; a version without an entry (a dev build) shows nothing.
    public static func whatsNew(app: String, lastSeen: String?, changelog: Changelog?) -> (show: ChangelogEntry?, remember: String?) {
        guard VersionMatch.parse(app) != nil, app != lastSeen else { return (nil, nil) }
        guard lastSeen != nil else { return (nil, app) }
        return (changelog?.entry(for: app), app)
    }

    /// The card wins over the bar for that open.
    public static func panelTip(whatsNew: ChangelogEntry?, bar: String?) -> PanelTip {
        if let whatsNew { return .whatsNew(whatsNew) }
        if let bar { return .update(version: bar) }
        return .none
    }

    // MARK: - Words

    public static func barText(_ version: String) -> String {
        L.t("cc-moss \(version) is ready to install", "cc-moss \(version) 可以更新了")
    }

    public static var seeWhatsNew: String { L.t("See what's new", "看看新功能") }
    public static var update: String { L.t("Update", "更新") }
    public static var fullChangelog: String { L.t("Full changelog", "完整更新记录") }
    public static var gotIt: String { L.t("Got it", "知道了") }

    public static func cardTitle(_ version: String) -> String {
        L.t("New in cc-moss \(version)", "cc-moss \(version) 的新变化")
    }

    /// The card's lines: the headline and up to three highlights.
    public static func highlights(_ entry: ChangelogEntry, lang: Lang = L.lang) -> [String] {
        entry.highlights.prefix(highlightLimit).map { $0.text(lang) }
    }
}

/// What the app remembers about versions, on this Mac: the version whose
/// "what's new" you saw, the version whose bar you closed, the version Mo
/// told you about.
public struct UpdateTipsMemory {
    static let lastSeenKey = "updateTips.lastSeenVersion"
    static let dismissedKey = "updateTips.barDismissedVersion"
    static let bubbledKey = "updateTips.bubbledVersion"

    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    public var lastSeen: String? {
        get { defaults.string(forKey: Self.lastSeenKey) }
        nonmutating set { defaults.set(newValue, forKey: Self.lastSeenKey) }
    }

    public var dismissed: String? {
        get { defaults.string(forKey: Self.dismissedKey) }
        nonmutating set { defaults.set(newValue, forKey: Self.dismissedKey) }
    }

    public var bubbled: String? {
        get { defaults.string(forKey: Self.bubbledKey) }
        nonmutating set { defaults.set(newValue, forKey: Self.bubbledKey) }
    }
}
