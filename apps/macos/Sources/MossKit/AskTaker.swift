import Foundation

/// "The app is here to take questions": while it runs, sessions wait a
/// while for an answer from the panel or Mo instead of only the terminal.
/// `cc-moss ask take --name desktop` counts for 90 seconds, so it is sent
/// every 60 (T1.3). Off when Settings › Notifications › "Answer questions
/// here" is off (`ask.enabled`).
@MainActor
public final class AskTaker {
    public static let name = "desktop"
    public static let interval: TimeInterval = 60

    private let beat: @MainActor () async -> Void
    /// Waits; false when the wait was cut short and the loop should end.
    private let sleep: @MainActor (TimeInterval) async -> Bool
    private var task: Task<Void, Never>?
    private var generation = 0

    /// Starts or stops the heartbeat.
    public var enabled: Bool {
        didSet {
            guard enabled != oldValue else { return }
            enabled ? start() : stop()
        }
    }

    public init(enabled: Bool = true,
                beat: @escaping @MainActor () async -> Void,
                sleep: @escaping @MainActor (TimeInterval) async -> Bool = AskTaker.realSleep) {
        self.enabled = enabled
        self.beat = beat
        self.sleep = sleep
    }

    public convenience init(client: MossClient, enabled: Bool = true) {
        self.init(enabled: enabled, beat: { try? await client.take(name: AskTaker.name) })
    }

    public var running: Bool { task != nil }

    /// Say "here" now, then every minute while enabled.
    public func start() {
        guard enabled, task == nil else { return }
        generation += 1
        let mine = generation
        task = Task { [weak self] in
            while let self, self.enabled, !Task.isCancelled {
                await self.beat()
                guard self.enabled, await self.sleep(Self.interval), !Task.isCancelled else { break }
            }
            if let self, self.generation == mine { self.task = nil }
        }
    }

    /// Stop saying "here"; sessions stop waiting within 90 seconds.
    public func stop() {
        task?.cancel()
        task = nil
    }

    /// Run the loop to its end; for tests.
    public func finished() async {
        await task?.value
    }

    public nonisolated static func realSleep(_ seconds: TimeInterval) async -> Bool {
        do {
            try await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            return true
        } catch {
            return false
        }
    }
}
