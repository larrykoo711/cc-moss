import AppKit
import AVFoundation
import MossKit

/// Mo's voice: the system speech synthesizer and one built-in sound
/// (DESIGN-pet §2.4, §6.4). No audio files of our own.
///
/// A new phrase drops whatever is still being said: no queue of stale news.
/// Rate and volume are the system's defaults.
@MainActor
final class Speaker: VoiceOutput {
    static let shared = Speaker()

    /// A short, soft system sound.
    static let chimeName = "Tink"

    /// System Settings › Accessibility › Spoken Content, where better voices are downloaded.
    static let downloadVoicesURL = "x-apple.systempreferences:com.apple.preference.universalaccess?SpeakableItems"

    private let synthesizer = AVSpeechSynthesizer()

    func say(_ text: String, voice: String?, language: String) {
        if synthesizer.isSpeaking { synthesizer.stopSpeaking(at: .immediate) }
        let utterance = AVSpeechUtterance(string: text)
        // The chosen voice was checked against the installed ones just now;
        // if it vanished in between, the system's voice for the language reads it.
        utterance.voice = voice.flatMap(AVSpeechSynthesisVoice.init(identifier:))
            ?? AVSpeechSynthesisVoice(language: language)
        synthesizer.speak(utterance)
    }

    func chime() {
        NSSound(named: NSSound.Name(Self.chimeName))?.play()
    }

    /// What the system has installed right now.
    static func installedVoices() -> [VoiceInfo] {
        AVSpeechSynthesisVoice.speechVoices().map { v in
            let quality: VoiceQuality = switch v.quality {
            case .premium: .premium
            case .enhanced: .enhanced
            default: .standard
            }
            return VoiceInfo(id: v.identifier, language: v.language, name: v.name, quality: quality,
                             novelty: v.voiceTraits.contains(.isNoveltyVoice) || v.voiceTraits.contains(.isPersonalVoice))
        }
    }
}
