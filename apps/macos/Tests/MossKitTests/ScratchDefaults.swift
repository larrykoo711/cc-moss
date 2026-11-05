import Foundation

/// Preferences for one test, kept out of `~/Library/Preferences`.
///
/// A suite named by a plain string is written to `~/Library/Preferences/<name>.plist`,
/// and `removePersistentDomain` only empties that file, it never deletes it, so every
/// test run used to leave a plist behind. A suite named by an absolute path is kept in
/// that file instead: here, one folder per test process in the temporary folder,
/// removed when the process exits (and cleared by macOS if it never gets the chance).
func scratchDefaults(_ label: String) -> UserDefaults {
    let file = ScratchDefaults.root.appendingPathComponent("\(label)-\(UUID().uuidString)")
    return UserDefaults(suiteName: file.path)!
}

enum ScratchDefaults {
    static let root: URL = {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("cc-moss-tests-\(getpid())", isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        atexit { try? FileManager.default.removeItem(at: ScratchDefaults.root) }
        return url
    }()
}
