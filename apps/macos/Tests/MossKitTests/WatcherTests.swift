import CoreServices
import Foundation
import Testing
@testable import MossKit

final class Counter: @unchecked Sendable {
    private let lock = NSLock()
    private var n = 0
    func bump() { lock.lock(); n += 1; lock.unlock() }
    var value: Int { lock.lock(); defer { lock.unlock() }; return n }
}

@Suite struct WatcherTests {
    func waitUntil(_ seconds: Double, _ ok: () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            if ok() { return true }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        return ok()
    }

    @Test func refreshesOnChange() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ccmoss-watch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let count = Counter()
        let watcher = try #require(DirectoryWatcher(path: dir.path, latency: 0.3) { count.bump() })
        defer { watcher.stop() }
        try await Task.sleep(nanoseconds: 300_000_000)
        try Data("x".utf8).write(to: dir.appendingPathComponent("registry.db-wal"))
        #expect(await waitUntil(2) { count.value >= 1 })
    }

    /// The temporary folder is under /var, a symlink to /private/var:
    /// FSEvents reports the real path, and the filter still sees paths under
    /// the folder it was given.
    @Test func theFilterSeesPathsUnderTheFolderAsGiven() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ccmoss-watch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        #expect(dir.path.hasPrefix("/var/"), "a folder behind a symlink")
        let given = dir.path
        let count = Counter()
        let watcher = try #require(DirectoryWatcher(path: given, latency: 0.3,
                                                    filter: { StateChange.matters($0, in: given) }) { count.bump() })
        defer { watcher.stop() }
        try await Task.sleep(nanoseconds: 300_000_000)
        try Data("x".utf8).write(to: dir.appendingPathComponent("registry.changed"))
        #expect(await waitUntil(2) { count.value >= 1 })
    }

    @Test func eventsAreGivenUnderTheFolderAsGiven() {
        #expect(DirectoryWatcher.rebase("/private/var/x/registry.changed", real: "/private/var/x", given: "/var/x")
            == "/var/x/registry.changed")
        #expect(DirectoryWatcher.rebase("/elsewhere/f", real: "/private/var/x", given: "/var/x") == "/elsewhere/f")
        #expect(DirectoryWatcher.rebase("/private/var/x", real: "/private/var/x", given: "/var/x") == "/var/x")
    }

    @Test func droppedEventsCountAsAChange() {
        #expect(DirectoryWatcher.mustRescan(FSEventStreamEventFlags(kFSEventStreamEventFlagMustScanSubDirs)))
        #expect(DirectoryWatcher.mustRescan(FSEventStreamEventFlags(kFSEventStreamEventFlagUserDropped)))
        #expect(DirectoryWatcher.mustRescan(FSEventStreamEventFlags(kFSEventStreamEventFlagKernelDropped)))
        #expect(!DirectoryWatcher.mustRescan(FSEventStreamEventFlags(kFSEventStreamEventFlagItemModified)))
    }

    @Test func coalescesBursts() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ccmoss-watch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let count = Counter()
        let watcher = try #require(DirectoryWatcher(path: dir.path, latency: 1.0) { count.bump() })
        defer { watcher.stop() }
        try await Task.sleep(nanoseconds: 300_000_000)
        let file = dir.appendingPathComponent("registry.db-wal")
        for i in 0..<20 {
            try Data("\(i)".utf8).write(to: file)
        }
        #expect(await waitUntil(3) { count.value >= 1 })
        try await Task.sleep(nanoseconds: 1_500_000_000)
        // FSEvents may cut a burst into a few batches on a busy machine (CI
        // has given three); what matters is that 20 writes don't become 20 calls.
        #expect(count.value <= 4, "20 writes in a burst arrive as a few calls, got \(count.value)")
    }

    @Test func singleFlightServesABurstWithOneExtraRun() async {
        let runs = Counter()
        let flight = SingleFlight {
            runs.bump()
            try? await Task.sleep(nanoseconds: 200_000_000)
        }
        await withTaskGroup(of: Void.self) { group in
            for _ in 0..<10 { group.addTask { await flight.request() } }
        }
        #expect(runs.value == 2)
    }
}
