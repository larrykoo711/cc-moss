import Testing
@testable import MossKit

/// Records what would have been said; nothing is ever played.
@MainActor private final class FakeOutput: VoiceOutput {
    var said: [(text: String, voice: String?, language: String)] = []
    var chimes = 0
    func say(_ text: String, voice: String?, language: String) { said.append((text, voice, language)) }
    func chime() { chimes += 1 }
}

@MainActor @Suite struct VoiceAnnouncerTests {
    private let voices = [
        VoiceInfo(id: "std", language: "en-US", name: "Plain", quality: .standard, novelty: false),
        VoiceInfo(id: "pre", language: "en-US", name: "Rich", quality: .premium, novelty: false),
    ]

    private func announce(_ a: inout VoiceAnnouncer, _ out: FakeOutput, voice: VoiceMode = .speak,
                          stored: String? = nil, at t: Double) {
        let installed = voices
        a.announce(kind: .needsYou, phrase: "job-a needs you", mode: .everyday, voice: voice, doNotDisturb: false,
                   storedVoice: stored, installed: { installed }, lang: .en, at: t, to: out)
    }

    @Test func speaksWithChosenVoice() {
        var a = VoiceAnnouncer()
        let out = FakeOutput()
        announce(&a, out, stored: "gone", at: 0)
        #expect(out.said.count == 1)
        #expect(out.said.first?.text == "job-a needs you")
        #expect(out.said.first?.voice == "pre")
        #expect(out.said.first?.language == "en-US")
        #expect(out.chimes == 0)
    }

    @Test func secondWithin30sIsDropped() {
        var a = VoiceAnnouncer()
        let out = FakeOutput()
        announce(&a, out, at: 0)
        announce(&a, out, at: 10)
        #expect(out.said.count == 1)
    }

    @Test func chimeOnlyChimes() {
        var a = VoiceAnnouncer()
        let out = FakeOutput()
        announce(&a, out, voice: .chime, at: 0)
        #expect(out.chimes == 1)
        #expect(out.said.isEmpty)
    }
}
