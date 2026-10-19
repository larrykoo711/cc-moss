import Foundation

/// `<state>/desktop/panel-opened`: touched every time the panel opens. The
/// daemon reads its time for the day's first checkup ("the first time you
/// open the panel each day"), whether or not questions are answered here.
public enum PanelOpenedMark {
    public static func path(stateDir: String) -> String {
        (stateDir as NSString).appendingPathComponent("desktop/panel-opened")
    }

    /// Make it (only you can read it) or move its time to `now`.
    public static func touch(stateDir: String, now: Date = Date()) throws {
        let fm = FileManager.default
        let file = path(stateDir: stateDir)
        let dir = (file as NSString).deletingLastPathComponent
        try fm.createDirectory(atPath: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        if !fm.fileExists(atPath: file) {
            guard fm.createFile(atPath: file, contents: nil, attributes: [.posixPermissions: 0o600]) else {
                throw CocoaError(.fileWriteUnknown)
            }
        }
        try fm.setAttributes([.modificationDate: now], ofItemAtPath: file)
    }
}
