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
    /// The folder as given, and as FSEvents names it (symlinks resolved:
    /// /var is /private/var). Event paths are handed to `filter` as given.
    private let given: String
    private let real: String

    public init?(path: String, latency: Double = 1.0, filter: @escaping @Sendable (String) -> Bool = { _ in true },
                 onChange: @escaping @Sendable () -> Void) {
        self.onChange = onChange
        self.filter = filter
        given = path
        real = Self.realPath(path)
        var context = FSEventStreamContext(
            version: 0,
            info: Unmanaged.passUnretained(self).toOpaque(),
            retain: nil, release: nil, copyDescription: nil
        )
        let callback: FSEventStreamCallback = { _, info, count, paths, flags, _ in
            guard let info else { return }
            let watcher = Unmanaged<DirectoryWatcher>.fromOpaque(info).takeUnretainedValue()
            let changed = (unsafeBitCast(paths, to: NSArray.self) as? [String] ?? [])
                .map { DirectoryWatcher.rebase($0, real: watcher.real, given: watcher.given) }
            // Events were dropped or merged into "look the folder over": it
            // may have changed in any way.
            let rescan = (0..<count).contains { DirectoryWatcher.mustRescan(flags[$0]) }
            if rescan || changed.count < count || changed.contains(where: watcher.filter) { watcher.onChange() }
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

    /// The folder with every symlink resolved, as FSEvents reports paths.
    static func realPath(_ path: String) -> String {
        guard let resolved = realpath(path, nil) else { return path }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    /// An event's path under the folder as it was given.
    static func rebase(_ path: String, real: String, given: String) -> String {
        guard real != given else { return path }
        if path == real { return given }
        let prefix = real.hasSuffix("/") ? real : real + "/"
        guard path.hasPrefix(prefix) else { return path }
        return (given.hasSuffix("/") ? given : given + "/") + path.dropFirst(prefix.count)
    }

    /// Flags that mean events were lost: look again, whatever the paths say.
    static func mustRescan(_ flags: FSEventStreamEventFlags) -> Bool {
        let lost = kFSEventStreamEventFlagMustScanSubDirs | kFSEventStreamEventFlagUserDropped
            | kFSEventStreamEventFlagKernelDropped | kFSEventStreamEventFlagRootChanged
        return flags & FSEventStreamEventFlags(lost) != 0
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

/// Like `SingleFlight`, but every request waits for a run that started
/// after it was made: a burst shares one more run, and nobody's request is
/// answered by a run that began before it (and may have missed its change).
public actor JoinedFlight {
    private let work: @Sendable () async -> Void
    private var running = false
    private var again = false
    private var started = 0
    private var finished = 0
    private var waiters: [(run: Int, done: CheckedContinuation<Void, Never>)] = []

    public init(_ work: @escaping @Sendable () async -> Void) {
        self.work = work
    }

    public func request() async {
        let run = started + 1
        if running {
            again = true
        } else {
            running = true
            Task { await loop() }
        }
        await withCheckedContinuation { waiters.append((run, $0)) }
    }

    private func loop() async {
        repeat {
            again = false
            started += 1
            await work()
            finished += 1
            let ready = waiters.filter { $0.run <= finished }
            waiters.removeAll { $0.run <= finished }
            ready.forEach { $0.done.resume() }
        } while again
        running = false
    }
}
