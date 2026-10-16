import Foundation

/// One picture of the menu bar icon: a state and which of its frames.
public struct IconFrame: Equatable, Sendable {
    public var state: IconState
    public var index: Int

    public init(state: IconState, index: Int) {
        self.state = state
        self.index = index
    }
}

/// Timing of the icon's two animations; everything else is a still.
public enum IconMotion {
    /// About 12 frames a second: smooth enough at 18 pt, cheap to run.
    public static let frameInterval: TimeInterval = 0.08

    /// How long one loop takes.
    public static func period(_ state: IconState) -> TimeInterval {
        switch state {
        case .working: 1.6 // a spot of light travels round the loop
        case .needsYou: 1.2 // a gentle pulse
        default: 0
        }
    }

    public static func frameCount(_ state: IconState) -> Int {
        let p = period(state)
        return p > 0 ? Int((p / frameInterval).rounded()) : 1
    }

    public static func animates(_ state: IconState) -> Bool { frameCount(state) > 1 }
}

/// Something that calls back at a fixed interval until stopped. The app
/// uses a run-loop timer; tests fire ticks by hand.
public protocol TickScheduler: AnyObject {
    func start(interval: TimeInterval, _ tick: @escaping () -> Void)
    func stop()
}

/// Decides which frame the menu bar icon shows, and runs a timer only while
/// that frame can change: an animated state, motion allowed, screens awake.
public final class IconAnimator {
    public private(set) var frame = IconFrame(state: .idle, index: 0)
    /// Called with every new frame, and only when it differs from the last.
    public var onFrame: ((IconFrame) -> Void)?

    public var state: IconState = .idle {
        didSet { if state != oldValue { restart() } }
    }

    /// System setting "Reduce motion": stills only, colours still change.
    public var reduceMotion = false {
        didSet { if reduceMotion != oldValue { restart() } }
    }

    /// Displays asleep or the screen locked: nobody is looking.
    public var screensAsleep = false {
        didSet { if screensAsleep != oldValue { restart() } }
    }

    public var isTicking: Bool { ticking }

    private let clock: () -> TimeInterval
    private let scheduler: TickScheduler
    private var startedAt: TimeInterval = 0
    private var ticking = false

    public init(clock: @escaping () -> TimeInterval, scheduler: TickScheduler) {
        self.clock = clock
        self.scheduler = scheduler
    }

    deinit { if ticking { scheduler.stop() } }

    private var shouldTick: Bool {
        IconMotion.animates(state) && !reduceMotion && !screensAsleep
    }

    /// Back to the state's first frame; start or stop the timer to match.
    private func restart() {
        startedAt = clock()
        show(IconFrame(state: state, index: 0))
        if shouldTick, !ticking {
            ticking = true
            scheduler.start(interval: IconMotion.frameInterval) { [weak self] in self?.tick() }
        } else if !shouldTick, ticking {
            ticking = false
            scheduler.stop()
        }
    }

    private func tick() {
        guard shouldTick else { return }
        let elapsed = max(0, clock() - startedAt)
        let index = Int((elapsed / IconMotion.frameInterval).rounded(.down)) % IconMotion.frameCount(state)
        show(IconFrame(state: state, index: index))
    }

    private func show(_ next: IconFrame) {
        guard next != frame else { return }
        frame = next
        onFrame?(next)
    }
}
