import Foundation
import MossKit

/// Looks at the state folder every few seconds, for when FSEvents doesn't
/// answer: one `stat` of the files a change touches, no cc-moss run.
@MainActor
final class StatePoller {
    /// How long the watcher gets to start before polling takes over.
    static let patience: TimeInterval = 3
    static let interval: TimeInterval = 2

    private var timer: Timer?
    private var last: [Date?]

    init(stateDir: String, onChange: @escaping @MainActor () -> Void) {
        let files = StateChange.polledFiles(in: stateDir)
        last = files.map(Self.modified)
        timer = Timer.scheduledTimer(withTimeInterval: Self.interval, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                let now = files.map(Self.modified)
                if now != self.last {
                    self.last = now
                    onChange()
                }
            }
        }
    }

    private static func modified(_ path: String) -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: path))?[.modificationDate] as? Date
    }

    deinit { timer?.invalidate() }
}
