import Foundation

/// Writes one on/off setting from a switch: one write at a time, in order,
/// and the last value asked for wins. After its write the setting is read
/// back, so the switch ends up showing what the config really holds, even
/// when a write failed. A quick on-then-off must never leave it on.
@MainActor
public final class SwitchWriter {
    public enum Outcome: Equatable {
        /// Already asked for: nothing to write.
        case unchanged
        /// A newer value was asked for meanwhile; that one will report.
        case superseded
        /// Written (or not: `failed`); `holds` is what the config holds now,
        /// nil if it couldn't be read.
        case done(holds: Bool?, failed: String?)
    }

    /// The last value asked for (or loaded).
    public private(set) var requested: Bool
    /// Writes asked for and not yet finished: reading the config afresh
    /// now would show a value about to change.
    public private(set) var pending = 0
    public var isWriting: Bool { pending > 0 }
    private var last: Task<Void, Never>?
    private let write: (Bool) async throws -> Void
    private let read: () async throws -> Bool
    private let describe: (any Error) -> String

    public init(holds: Bool,
                write: @escaping (Bool) async throws -> Void,
                read: @escaping () async throws -> Bool,
                describe: @escaping (any Error) -> String = { String(describing: $0) }) {
        requested = holds
        self.write = write
        self.read = read
        self.describe = describe
    }

    public func set(_ value: Bool) async -> Outcome {
        guard value != requested else { return .unchanged }
        requested = value
        pending += 1
        defer { pending -= 1 }
        // Writes queue up behind each other, oldest first.
        let before = last
        let task = Task<Outcome, Never> { @MainActor in
            await before?.value
            guard requested == value else { return .superseded }
            var failed: String?
            do { try await write(value) } catch { failed = describe(error) }
            guard requested == value else { return .superseded }
            let holds = try? await read()
            if let holds { requested = holds }
            return .done(holds: holds, failed: failed)
        }
        last = Task { @MainActor in _ = await task.value }
        return await task.value
    }
}
