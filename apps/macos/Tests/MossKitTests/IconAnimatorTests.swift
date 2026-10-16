import Foundation
import Testing
@testable import MossKit

/// A scheduler that never fires on its own: tests call `fire()`.
final class ManualTicks: TickScheduler {
    private(set) var running = false
    private(set) var interval: TimeInterval?
    private(set) var starts = 0
    private var tick: (() -> Void)?

    func start(interval: TimeInterval, _ tick: @escaping () -> Void) {
        running = true
        starts += 1
        self.interval = interval
        self.tick = tick
    }

    func stop() {
        running = false
        tick = nil
    }

    func fire() { tick?() }
}

final class FakeClock {
    var now: TimeInterval = 1000
}

@Suite struct IconAnimatorTests {
    func make(_ state: IconState = .idle) -> (IconAnimator, ManualTicks, FakeClock) {
        let ticks = ManualTicks()
        let clock = FakeClock()
        let a = IconAnimator(clock: { clock.now }, scheduler: ticks)
        a.state = state
        return (a, ticks, clock)
    }

    @Test func timerOnlyForAnimatedStates() {
        let (a, ticks, _) = make()
        for still in [IconState.idle, .waiting, .interrupted] {
            a.state = still
            #expect(!ticks.running, "\(still) should not tick")
            #expect(a.frame == IconFrame(state: still, index: 0))
        }
        for moving in [IconState.working, .needsYou] {
            a.state = moving
            #expect(ticks.running, "\(moving) should tick")
            #expect(ticks.interval == IconMotion.frameInterval)
        }
        a.state = .idle
        #expect(!ticks.running)
    }

    @Test func framesFollowTheClock() {
        let (a, ticks, clock) = make(.working)
        #expect(a.frame == IconFrame(state: .working, index: 0))
        clock.now += IconMotion.frameInterval * 3 + 0.001
        ticks.fire()
        #expect(a.frame.index == 3)
        // One full period later the loop is back where it was.
        clock.now += IconMotion.period(.working)
        ticks.fire()
        #expect(a.frame.index == 3)
    }

    @Test func reportsOnlyChangedFrames() {
        let (a, ticks, clock) = make(.needsYou)
        var seen: [IconFrame] = []
        a.onFrame = { seen.append($0) }
        ticks.fire() // same instant: nothing new
        clock.now += IconMotion.frameInterval + 0.001
        ticks.fire()
        #expect(seen == [IconFrame(state: .needsYou, index: 1)])
    }

    @Test func reduceMotionStopsFrames() {
        let (a, ticks, clock) = make(.working)
        a.reduceMotion = true
        #expect(!ticks.running)
        clock.now += 1
        ticks.fire()
        // The state still shows, as its still frame.
        #expect(a.frame == IconFrame(state: .working, index: 0))
        a.state = .needsYou
        #expect(!ticks.running)
        #expect(a.frame == IconFrame(state: .needsYou, index: 0))
        a.reduceMotion = false
        #expect(ticks.running)
    }

    @Test func pausesWhileScreensSleep() {
        let (a, ticks, clock) = make(.working)
        a.screensAsleep = true
        #expect(!ticks.running)
        // A state change while asleep must not wake the timer either.
        a.state = .needsYou
        #expect(!ticks.running)
        #expect(a.frame.state == .needsYou)
        clock.now += 5
        a.screensAsleep = false
        #expect(ticks.running)
        #expect(a.frame == IconFrame(state: .needsYou, index: 0))
    }

    @Test func sameStateDoesNotRestartTheLoop() {
        let (a, ticks, clock) = make(.working)
        clock.now += IconMotion.frameInterval * 2 + 0.001
        ticks.fire()
        a.state = .working // a snapshot refresh with nothing new
        #expect(ticks.starts == 1)
        #expect(a.frame.index == 2)
    }

    @Test func frameCountsMatchTheirPeriods() {
        // Two updates a second at most: 12 steps round the loop, a two-beat pulse.
        #expect(IconMotion.frameInterval >= 0.5)
        #expect(IconMotion.frameCount(.working) == 12)
        #expect(IconMotion.frameCount(.needsYou) == 2)
        #expect(IconMotion.frameCount(.idle) == 1)
    }
}
