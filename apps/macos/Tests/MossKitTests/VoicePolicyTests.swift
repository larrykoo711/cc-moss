import Foundation
import Testing
@testable import MossKit

@Suite(.serialized) struct VoicePolicyTests {
    private func cue(_ kind: AlertKind, mode: PetInteraction = .everyday, voice: VoiceMode = .speak,
                     dnd: Bool = false, at t: TimeInterval = 100, policy: inout VoicePolicy) -> VoiceCue {
        policy.cue(for: kind, phrase: "job-a needs you", mode: mode, voice: voice, doNotDisturb: dnd, at: t)
    }

    private func fresh(_ kind: AlertKind, mode: PetInteraction = .everyday, voice: VoiceMode = .speak, dnd: Bool = false) -> VoiceCue {
        var policy = VoicePolicy()
        return cue(kind, mode: mode, voice: voice, dnd: dnd, policy: &policy)
    }

    @Test func offSaysNothing() {
        for kind in AlertKind.allCases {
            #expect(fresh(kind, mode: .active, voice: .off) == .nothing)
        }
    }

    @Test func defaultIsOff() {
        let defaults = UserDefaults(suiteName: "voice-policy-tests-\(UUID().uuidString)")!
        #expect(VoiceMode(defaults: defaults) == .off)
    }

    @Test func quietOnlyNeedsYou() {
        #expect(fresh(.needsYou, mode: .quiet) == .speak("job-a needs you"))
        #expect(fresh(.interrupted, mode: .quiet) == .nothing)
        #expect(fresh(.restored, mode: .quiet) == .nothing)
        #expect(fresh(.finished, mode: .quiet) == .nothing)
    }

    @Test func everydayAddsInterrupted() {
        #expect(fresh(.needsYou) != .nothing)
        #expect(fresh(.interrupted) != .nothing)
        #expect(fresh(.restored) == .nothing)
        #expect(fresh(.finished) == .nothing)
    }

    @Test func activeAddsRestoredAndFinished() {
        for kind in AlertKind.allCases {
            #expect(fresh(kind, mode: .active) != .nothing)
        }
    }

    @Test func doNotDisturbMutes() {
        for kind in AlertKind.allCases {
            #expect(fresh(kind, mode: .active, dnd: true) == .nothing)
            #expect(fresh(kind, mode: .active, voice: .chime, dnd: true) == .nothing)
        }
    }

    @Test func chimeModeChimes() {
        #expect(fresh(.needsYou, voice: .chime) == .chime)
        #expect(fresh(.restored, voice: .chime) == .nothing) // everyday: not a kind that makes a sound
    }

    @Test func atMostOncePer30s() {
        var policy = VoicePolicy()
        #expect(cue(.needsYou, at: 100, policy: &policy) != .nothing)
        #expect(cue(.interrupted, at: 110, policy: &policy) == .nothing)
        #expect(cue(.needsYou, at: 129.9, policy: &policy) == .nothing)
        #expect(cue(.needsYou, at: 130, policy: &policy) != .nothing)
    }

    @Test func mutedDoesNotStartTheClock() {
        var policy = VoicePolicy()
        #expect(cue(.needsYou, dnd: true, at: 100, policy: &policy) == .nothing)
        #expect(cue(.restored, at: 105, policy: &policy) == .nothing) // everyday: a silent kind
        #expect(cue(.needsYou, at: 110, policy: &policy) != .nothing)
    }

    @Test func phraseIsTheTitleOnly() {
        L.lang = .en
        defer { L.lang = .detect() }
        let alert = SessionAlert(id: 1, slotId: "A", kind: .needsYou, name: "job-a", reason: "question",
                                 at: Date(timeIntervalSince1970: 0))
        let phrase = VoicePolicy.phrase(PetMessage(alert), names: ["job-a"])
        #expect(phrase == "job-a has a question")
        #expect(!phrase.contains("answer")) // the body is never read
    }

    @Test func phraseFollowsLanguage() {
        let alert = SessionAlert(id: 1, slotId: "A", kind: .interrupted, name: "job-a", at: Date(timeIntervalSince1970: 0))
        defer { L.lang = .detect() }
        L.lang = .zh
        #expect(VoicePolicy.phrase(PetMessage(alert), names: ["job-a"]) == "job-a 断了")
        L.lang = .en
        #expect(VoicePolicy.phrase(PetMessage(alert), names: ["job-a"]) == "job-a dropped")
    }
}
