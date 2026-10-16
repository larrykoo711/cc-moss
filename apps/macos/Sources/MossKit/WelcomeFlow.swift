import Foundation

/// The first-run welcome (A22): a few steps, each doing its job on the
/// spot. Steps already taken care of (hooks in place, notifications allowed)
/// are ticked; any step, or the whole thing, can be skipped. Shown once.
public struct WelcomeFlow: Equatable, Sendable {
    public enum Step: Int, CaseIterable, Sendable {
        case welcome, tools, startup, terminal, mo, done
    }

    public enum StepState: Equatable, Sendable {
        case pending, done, skipped
    }

    public private(set) var step: Step = .welcome
    private var states: [Step: StepState] = [:]
    /// Pressed Continue on the last step.
    public private(set) var isFinished = false

    public init() {}

    public var canGoBack: Bool { step != .welcome }

    /// 1-based position and the number of steps, for the dots.
    public var progress: (Int, Int) { (step.rawValue + 1, Step.allCases.count) }

    public func state(of s: Step) -> StepState { states[s] ?? .pending }

    public mutating func next() {
        // Continuing past a step counts as doing it (unless it was skipped).
        if step != .welcome, states[step] == nil { states[step] = .done }
        guard let following = Step(rawValue: step.rawValue + 1) else {
            isFinished = true
            return
        }
        step = following
    }

    public mutating func back() {
        if let previous = Step(rawValue: step.rawValue - 1) { step = previous }
    }

    /// "Skip this step": on to the next one, marked as skipped.
    public mutating func skipStep() {
        if states[step] != .done { states[step] = .skipped }
        if let following = Step(rawValue: step.rawValue + 1) { step = following } else { isFinished = true }
    }

    /// Steps the machine already satisfies are ticked; a tick stays.
    public mutating func satisfied(_ steps: Set<Step>) {
        for s in steps { states[s] = .done }
    }

    // MARK: - Once

    static let seenKey = "welcomeSeen"

    public static func shouldShow(_ defaults: UserDefaults) -> Bool { !defaults.bool(forKey: seenKey) }

    /// Finished or skipped: don't show it at launch again.
    public static func markSeen(_ defaults: UserDefaults) { defaults.set(true, forKey: seenKey) }
}
