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
        #expect(count.value <= 2, "20 writes in a burst arrive as one or two calls, got \(count.value)")
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
