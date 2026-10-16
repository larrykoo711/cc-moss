import Foundation

/// What Mo is doing (DESIGN-pet §4).
public enum PetPose: String, CaseIterable, Sendable {
    case idle, working, needsYou, interrupted, happy, sleeping

    /// The pose the sessions call for. Sleeping wins (do not disturb is the
    /// user's own word), then the same order as the menu bar icon:
    /// needs you > interrupted > working > idle. `happy` is never a resting
    /// pose: it is a show (`PetStage`).
    public static func resting(_ sessions: [Session], doNotDisturb: Bool) -> PetPose {
        if doNotDisturb { return .sleeping }
        switch IconState(sessions) {
        case .needsYou: return .needsYou
        case .interrupted: return .interrupted
        case .working: return .working
        case .waiting, .idle: return .idle
        }
    }

    public var title: String {
        switch self {
        case .idle: L.t("Idle", "待机")
        case .working: L.t("Working", "在忙")
        case .needsYou: L.t("Needs you", "需要你")
        case .interrupted: L.t("Interrupted", "断了")
        case .happy: L.t("Back / done", "找回 / 完成")
        case .sleeping: L.t("Do not disturb", "勿扰")
        }
    }
}

/// The resting pose plus a one-off show (happy) that plays over it and then
/// hands back to whatever is true by then.
public struct PetStage: Equatable, Sendable {
    /// One hop (0.7 s) and two seconds of smiling.
    public static let happyDuration = 2.7

    public private(set) var resting: PetPose = .idle
    private var happyUntil: TimeInterval?

    public init() {}

    public mutating func rest(_ pose: PetPose) {
        resting = pose
        // Do not disturb ends the show at once.
        if pose == .sleeping { happyUntil = nil }
    }

    /// Start the happy show; false (and nothing happens) while asleep.
    @discardableResult
    public mutating func celebrate(at now: TimeInterval) -> Bool {
        guard resting != .sleeping else { return false }
        happyUntil = now + Self.happyDuration
        return true
    }

    /// When the current show ends, if one is on.
    public var performanceEnds: TimeInterval? { happyUntil }

    public func pose(at now: TimeInterval) -> PetPose {
        if let end = happyUntil, now < end { return .happy }
        return resting
    }
}

/// How visible Mo is (DESIGN-pet §6.5): translucent while nothing is going
/// on, fully there while it wants you or the pointer is on it, and back to
/// translucent a little after both stop.
public struct PetPresence: Equatable, Sendable {
    public static let rest = 0.55
    public static let fade = 0.3
    /// How long Mo stays fully visible after the pointer leaves or the matter is handled.
    public static let linger = 2.0

    private var engaged = false
    private var releasedAt: TimeInterval = -.infinity

    public init() {}

    /// Poses in which Mo is trying to get your attention.
    public static func wantsYou(_ pose: PetPose) -> Bool {
        pose == .needsYou || pose == .interrupted || pose == .happy
    }

    public mutating func update(pose: PetPose, hovering: Bool, at now: TimeInterval) {
        let next = hovering || Self.wantsYou(pose)
        if engaged, !next { releasedAt = now }
        engaged = next
    }

    /// When it should go back to translucent, if that is still to come.
    public var fadeBackAt: TimeInterval? {
        engaged || releasedAt == -.infinity ? nil : releasedAt + Self.linger
    }

    public func opacity(at now: TimeInterval) -> Double {
        if engaged || now < releasedAt + Self.linger { return 1 }
        return Self.rest
    }
}

/// Mo's continuous animations. Each runs in the render server as a
/// repeating layer animation; the app does nothing per frame.
public enum PetLoop: CaseIterable, Sendable {
    /// Idle: scale 1 → 1.025 and back, 4 s.
    case breathe
    /// Idle: a blink every 6–12 s (a one-shot started by a timer).
    case blink
    /// Working: the gap orbits the eyes, 1.6 s a lap.
    case flow
    /// Sleeping: a zzz drifts up every 3 s.
    case zzz

    /// Loops never ask for more than 30 frames a second; slow ones for less.
    public var maxFramesPerSecond: Float {
        switch self {
        case .flow: 30
        case .breathe, .zzz: 15
        case .blink: 30
        }
    }
}

public enum PetLoops {
    /// Two minutes of nothing happening and Mo holds still.
    public static let stillAfter: TimeInterval = 120

    /// Which loops should be on now. `since` is when the pose began;
    /// `visible` is false when the window is covered or the screens sleep.
    public static func running(pose: PetPose, since: TimeInterval, now: TimeInterval,
                               reduceMotion: Bool, visible: Bool) -> Set<PetLoop> {
        guard !reduceMotion, visible else { return [] }
        if let still = stillAt(pose: pose, since: since), now >= still { return [] }
        switch pose {
        case .idle: return [.breathe, .blink]
        case .working: return [.flow]
        case .sleeping: return [.zzz]
        case .needsYou, .interrupted, .happy: return []
        }
    }

    /// When the loops of a quiet pose stop; nil for a pose that keeps going
    /// (working shows that Claude is busy).
    public static func stillAt(pose: PetPose, since: TimeInterval) -> TimeInterval? {
        pose == .idle || pose == .sleeping ? since + stillAfter : nil
    }
}
