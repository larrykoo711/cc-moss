import AppKit
import ServiceManagement

/// "Open at Login". The system's login items (`SMAppService`) refuse an
/// ad-hoc signed app ("Invalid argument"), which is what cc-moss is, so when
/// they say no it falls back to a per-user launch agent that opens the app
/// at login. Turning it off removes whichever one is in use.
enum LoginItem {
    static let label = "io.github.larrykoo711.cc-moss.login"

    static var agentURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents/\(label).plist")
    }

    /// The system's last answer. Asking it goes through a background
    /// service that can stall for good; asked on the main thread, the
    /// Settings window froze opening (2026-10-20). So it is asked off it
    /// (`refresh`), and this never waits.
    @MainActor private static var systemSays = false

    /// On, as far as is known now: never blocks.
    @MainActor static var isEnabled: Bool {
        systemSays || FileManager.default.fileExists(atPath: agentURL.path)
    }

    /// Ask the system again, off the main thread; no answer within 2 s
    /// keeps the last one.
    @MainActor @discardableResult
    static func refresh() async -> Bool {
        if let said = await offMain({ SMAppService.mainApp.status == .enabled }) { systemSays = said }
        return isEnabled
    }

    /// Turn it on or off, off the main thread, then ask again.
    @MainActor
    static func set(_ on: Bool) async throws {
        let outcome: Result<Void, Error>? = await offMain {
            Result {
                if on {
                    do {
                        try SMAppService.mainApp.register()
                    } catch {
                        try writeAgent()
                    }
                } else {
                    try? SMAppService.mainApp.unregister()
                    if FileManager.default.fileExists(atPath: agentURL.path) {
                        try FileManager.default.removeItem(at: agentURL)
                    }
                }
            }
        }
        // No answer: the system's service is stuck; the launch agent still works.
        if outcome == nil, on { try writeAgent() }
        try outcome?.get()
        await refresh()
    }

    /// `work` on a background thread; nil when it hasn't answered in
    /// `seconds` (the thread is left to finish on its own).
    private static func offMain<T: Sendable>(seconds: Double = 2, _ work: @escaping @Sendable () -> T) async -> T? {
        await withCheckedContinuation { (done: CheckedContinuation<T?, Never>) in
            let once = Once(done)
            DispatchQueue.global(qos: .userInitiated).async { once.resume(work()) }
            DispatchQueue.global().asyncAfter(deadline: .now() + seconds) { once.resume(nil) }
        }
    }

    /// Resumes a continuation the first time only.
    private final class Once<T: Sendable>: @unchecked Sendable {
        private let lock = NSLock()
        private var continuation: CheckedContinuation<T?, Never>?
        init(_ c: CheckedContinuation<T?, Never>) { continuation = c }
        func resume(_ value: T?) {
            lock.lock()
            let c = continuation
            continuation = nil
            lock.unlock()
            c?.resume(returning: value)
        }
    }

    /// `open -g` starts the app in the background at login, from wherever
    /// it lives now; nothing runs as root and nothing is loaded until then.
    private static func writeAgent() throws {
        let plist: [String: Any] = [
            "Label": label,
            "ProgramArguments": ["/usr/bin/open", "-g", Bundle.main.bundlePath],
            "RunAtLoad": true,
            "LimitLoadToSessionType": "Aqua",
        ]
        let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
        try FileManager.default.createDirectory(at: agentURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: agentURL, options: .atomic)
    }
}
