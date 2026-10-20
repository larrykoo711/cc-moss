import Foundation

/// How Mo makes a sound when it has something for you (Settings › Mo ›
/// Voice, DESIGN-pet §6.4). Off unless you turn it on.
public enum VoiceMode: String, CaseIterable, Sendable {
    case off
    /// One built-in system sound.
    case chime
    /// The message title, read aloud by the system's speech synthesizer.
    case speak

    public static let key = "petVoice"

    public init(defaults: UserDefaults) {
        self = defaults.string(forKey: Self.key).flatMap(VoiceMode.init(rawValue:)) ?? .off
    }
}

/// What to play for one alert.
public enum VoiceCue: Equatable, Sendable {
    case nothing
    case chime
    case speak(String)
}

/// When Mo makes a sound, and what it says (DESIGN-pet §5, §6.4, §6.5).
///
/// - Which kinds, by interaction mode: quiet only "needs you"; everyday adds
///   "dropped"; active adds "back" and "done".
/// - Do not disturb: silent.
/// - At most one sound every 30 s, chime or speech alike.
/// - Only the bubble's title (a session name and our own words), never
///   anything from the session itself.
public struct VoicePolicy: Sendable {
    public static let gap: TimeInterval = 30

    /// When Mo last made a sound.
    public private(set) var lastAt: TimeInterval?

    public init() {}

    public static func sounds(_ kind: AlertKind, in mode: PetInteraction) -> Bool {
        switch mode {
        case .quiet: kind == .needsYou
        case .everyday: kind == .needsYou || kind == .interrupted
        case .active: kind != .update // news, not a call: never out loud
        }
    }

    /// The words to say for a message: its title, in the current language.
    public static func phrase(_ message: PetMessage, names: [String]) -> String {
        message.text(names: names).title
    }

    public mutating func cue(for kind: AlertKind, phrase: String, mode: PetInteraction, voice: VoiceMode,
                             doNotDisturb: Bool, at t: TimeInterval) -> VoiceCue {
        guard voice != .off, !doNotDisturb, Self.sounds(kind, in: mode) else { return .nothing }
        if let lastAt, t - lastAt < Self.gap { return .nothing }
        lastAt = t
        return voice == .chime ? .chime : .speak(phrase)
    }
}

/// The system's three voice qualities; better ones are downloaded by the user.
public enum VoiceQuality: Int, Comparable, Sendable {
    case standard = 1
    case enhanced
    case premium

    public static func < (a: Self, b: Self) -> Bool { a.rawValue < b.rawValue }

    /// The tag beside a voice's name in Settings; nil for the plain ones.
    public var tag: String? {
        switch self {
        case .standard: nil
        case .enhanced: L.t("Enhanced", "增强")
        case .premium: L.t("Premium", "高级")
        }
    }
}

/// One installed voice, as the system describes it.
public struct VoiceInfo: Equatable, Identifiable, Sendable {
    public var id: String
    /// A BCP 47 tag such as `zh-CN` or `en-US`.
    public var language: String
    public var name: String
    public var quality: VoiceQuality
    /// Novelty voices (bells, whispers) are not for reading messages.
    public var novelty: Bool

    public init(id: String, language: String, name: String, quality: VoiceQuality, novelty: Bool) {
        self.id = id
        self.language = language
        self.name = name
        self.quality = quality
        self.novelty = novelty
    }
}

/// Which installed voice reads in the interface language (DESIGN-pet §2.4).
/// Voices can be deleted in System Settings at any time, so the choice is
/// checked against what is installed every time Mo speaks.
public enum VoicePicker {
    /// The identifier chosen in Settings; absent: the best one.
    public static let key = "petVoiceId"

    /// The tag the system falls back to when no voice of ours is installed.
    public static func languageCode(_ lang: Lang) -> String {
        lang == .zh ? "zh-CN" : "en-US"
    }

    /// The voices for `lang`, best first: Mandarin before other Chinese,
    /// then premium, enhanced, standard, then the main region, then name.
    public static func installed(_ all: [VoiceInfo], for lang: Lang) -> [VoiceInfo] {
        let prefix = lang == .zh ? "zh" : "en"
        let main = languageCode(lang).lowercased()
        func normal(_ v: VoiceInfo) -> String { v.language.replacingOccurrences(of: "_", with: "-").lowercased() }
        func family(_ v: VoiceInfo) -> Int {
            // zh-HK is Cantonese: it would read Mandarin text in another dialect.
            lang == .zh && normal(v).hasPrefix("zh-hk") ? 0 : 1
        }
        return all
            .filter { !$0.novelty && normal($0).hasPrefix(prefix) }
            .sorted { a, b in
                if family(a) != family(b) { return family(a) > family(b) }
                if a.quality != b.quality { return a.quality > b.quality }
                let am = normal(a) == main, bm = normal(b) == main
                if am != bm { return am }
                return a.name.localizedStandardCompare(b.name) == .orderedAscending
            }
    }

    /// The stored voice if it is still installed and speaks `lang`, else the
    /// best one for `lang`; nil when there is none (the system then picks by language).
    public static func choose(stored: String?, from all: [VoiceInfo], for lang: Lang) -> VoiceInfo? {
        let candidates = installed(all, for: lang)
        if let stored, let kept = candidates.first(where: { $0.id == stored }) { return kept }
        return candidates.first
    }
}

/// Where sounds go: the speech synthesizer in the app, a recorder in tests.
@MainActor
public protocol VoiceOutput: AnyObject {
    /// Say `text`, dropping anything still being said. `voice` nil: the
    /// system's voice for `language`.
    func say(_ text: String, voice: String?, language: String)
    func chime()
}

/// The policy, the voice choice and the output, put together for one alert.
public struct VoiceAnnouncer: Sendable {
    public private(set) var policy = VoicePolicy()

    public init() {}

    @MainActor
    public mutating func announce(kind: AlertKind, phrase: String, mode: PetInteraction, voice: VoiceMode,
                                  doNotDisturb: Bool, storedVoice: String?, installed: () -> [VoiceInfo],
                                  lang: Lang = L.lang, at t: TimeInterval, to output: VoiceOutput) {
        switch policy.cue(for: kind, phrase: phrase, mode: mode, voice: voice, doNotDisturb: doNotDisturb, at: t) {
        case .nothing:
            break
        case .chime:
            output.chime()
        case let .speak(text):
            let chosen = VoicePicker.choose(stored: storedVoice, from: installed(), for: lang)
            output.say(text, voice: chosen?.id, language: VoicePicker.languageCode(lang))
        }
    }
}
