import CoreServices
import Foundation

/// Calls `onChange` when something in a folder changes that `filter` cares
/// about (it gets each changed file's path). FSEvents already gathers
/// bursts: every change within `latency` seconds arrives as one call.
/// The hooks write the registry on every prompt, so this is how the app
/// hears about it without polling.
public final class DirectoryWatcher: @unchecked Sendable {
    private var stream: FSEventStreamRef?
    private let queue = DispatchQueue(label: "cc-moss.watcher")
    private let onChange: @Sendable () -> Void
    private let filter: @Sendable (String) -> Bool

    public init?(path: String, latency: Double = 1.0, filter: @escaping @Sendable (String) -> Bool = { _ in true },
                 onChange: @escaping @Sendable () -> Void) {
        self.onChange = onChange
        self.filter = filter
        var context = FSEventStreamContext(
            version: 0,
            info: Unmanaged.passUnretained(self).toOpaque(),
            retain: nil, release: nil, copyDescription: nil
        )
        let callback: FSEventStreamCallback = { _, info, count, paths, _, _ in
            guard let info else { return }
            let watcher = Unmanaged<DirectoryWatcher>.fromOpaque(info).takeUnretainedValue()
            let changed = unsafeBitCast(paths, to: NSArray.self) as? [String] ?? []
            if changed.count < count || changed.contains(where: watcher.filter) { watcher.onChange() }
        }
        guard let stream = FSEventStreamCreate(
            nil, callback, &context, [path] as CFArray,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow), latency,
            FSEventStreamCreateFlags(kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer
                                     | kFSEventStreamCreateFlagUseCFTypes)
        ) else { return nil }
        self.stream = stream
        FSEventStreamSetDispatchQueue(stream, queue)
        guard FSEventStreamStart(stream) else {
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
            self.stream = nil
            return nil
        }
    }

    public func stop() {
        guard let stream else { return }
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
        self.stream = nil
    }

    deinit { stop() }
}

/// Runs `work` at most once at a time. A request that arrives while it runs
/// is remembered and served by exactly one more run afterwards, so a burst of
/// changes costs two runs, not one per change.
public actor SingleFlight {
    private var running = false
    private var again = false
    private let work: @Sendable () async -> Void

    public init(_ work: @escaping @Sendable () async -> Void) {
        self.work = work
    }

    public func request() async {
        if running {
            again = true
            return
        }
        running = true
        repeat {
            again = false
            await work()
        } while again
        running = false
    }
}
