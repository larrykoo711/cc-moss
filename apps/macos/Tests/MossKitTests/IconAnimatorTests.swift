import Foundation
import Testing
@testable import MossKit

/// Records what the animator asks for instead of animating.
final class FakeMotion: MotionDriver {
    private(set) var playing: IconState?
    private(set) var starts: [IconState] = []
    private(set) var stops = 0

    func startMotion(_ state: IconState) {
        #expect(playing == nil, "started \(state) while \(String(describing: playing)) was playing")
        playing = state
        starts.append(state)
    }

    func stopMotion() {
        playing = nil
        stops += 1
    }
}

@Suite struct IconAnimatorTests {
    func make(_ state: IconState = .idle) -> (IconAnimator, FakeMotion) {
        let motion = FakeMotion()
        let a = IconAnimator(driver: motion)
        a.state = state
        return (a, motion)
    }

    @Test func timerOnlyForAnimatedStates() {
        let (a, motion) = make()
        for still in [IconState.idle, .waiting, .interrupted] {
            a.state = still
            #expect(motion.playing == nil, "\(still) should not move")
        }
        for moving in [IconState.working, .needsYou] {
            a.state = moving
            #expect(motion.playing == moving)
        }
        a.state = .idle
        #expect(motion.playing == nil)
        #expect(a.moving == nil)
    }

    @Test func switchingAnimatedStatesSwapsTheAnimation() {
        let (a, motion) = make(.working)
        a.state = .needsYou
        #expect(motion.starts == [.working, .needsYou])
        #expect(motion.stops == 1)
        #expect(motion.playing == .needsYou)
    }

    @Test func reduceMotionStopsFrames() {
        let (a, motion) = make(.working)
        a.reduceMotion = true
        #expect(motion.playing == nil)
        // The state still changes (its colour and still form do), without motion.
        a.state = .needsYou
        #expect(motion.playing == nil)
        #expect(a.state == .needsYou)
        a.reduceMotion = false
        #expect(motion.playing == .needsYou)
    }

    @Test func pausesWhileScreensSleep() {
        let (a, motion) = make(.working)
        a.screensAsleep = true
        #expect(motion.playing == nil)
        // A state change while asleep must not start anything either.
        a.state = .needsYou
        #expect(motion.playing == nil)
        a.screensAsleep = false
        #expect(motion.playing == .needsYou)
    }

    @Test func sameStateDoesNotRestartTheLoop() {
        let (a, motion) = make(.working)
        a.state = .working // a snapshot refresh with nothing new
        a.reduceMotion = false
        a.screensAsleep = false
        #expect(motion.starts == [.working])
        #expect(motion.stops == 0)
    }

    @Test func samplesForReview() {
        // Ten a second: 1.6 s of flow, 1.2 s of pulse.
        #expect(IconMotion.sampleCount(.working) == 16)
        #expect(IconMotion.sampleCount(.needsYou) == 12)
        #expect(IconMotion.sampleCount(.idle) == 1)
        #expect(IconFrame(state: .working, index: 4).progress == 0.25)
    }
}
