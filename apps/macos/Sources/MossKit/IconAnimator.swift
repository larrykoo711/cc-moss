import Foundation

/// One picture of the menu bar icon: a state and, for the animated ones,
/// which of `IconMotion.sampleCount` evenly spaced moments of its loop.
/// The live icon animates continuously; samples are for stills and review.
public struct IconFrame: Hashable, Sendable {
    public var state: IconState
    public var index: Int

    public init(state: IconState, index: Int) {
        self.state = state
        self.index = index
    }

    /// How far through the loop this frame is, 0..<1.
    public var progress: Double {
        Double(index) / Double(IconMotion.sampleCount(state))
    }
}

/// Timing of the icon's two animations; everything else is a still.
public enum IconMotion {
    /// How long one loop takes.
    public static func period(_ state: IconState) -> TimeInterval {
        switch state {
        case .working: 1.6 // a spot of light travels round the loop
        case .needsYou: 1.2 // a gentle pulse
        default: 0
        }
    }

    public static func animates(_ state: IconState) -> Bool { period(state) > 0 }

    /// Moments of the loop drawn for review sheets: ten a second.
    public static func sampleCount(_ state: IconState) -> Int {
        animates(state) ? Int((period(state) * 10).rounded()) : 1
    }
}

/// Plays a state's animation on its own (Core Animation in the app) until
/// told to stop: the app does no work per frame.
public protocol MotionDriver: AnyObject {
    func startMotion(_ state: IconState)
    func stopMotion()
}

/// Decides when the menu bar icon moves: an animated state, motion allowed,
/// screens awake. Otherwise the driver is stopped and the icon is a still.
public final class IconAnimator {
    public var state: IconState = .idle { didSet { reconcile() } }

    /// System setting "Reduce motion": stills only, colours still change.
    public var reduceMotion = false { didSet { reconcile() } }

    /// Displays asleep or the screen locked: nobody is looking.
    public var screensAsleep = false { didSet { reconcile() } }

    /// The state whose animation is playing, if any.
    public private(set) var moving: IconState?

    private let driver: MotionDriver

    public init(driver: MotionDriver) {
        self.driver = driver
    }

    deinit { if moving != nil { driver.stopMotion() } }

    private func reconcile() {
        let wanted: IconState? = IconMotion.animates(state) && !reduceMotion && !screensAsleep ? state : nil
        guard wanted != moving else { return }
        if moving != nil { driver.stopMotion() }
        moving = wanted
        if let wanted { driver.startMotion(wanted) }
    }
}
