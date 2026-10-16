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

    static var isEnabled: Bool {
        SMAppService.mainApp.status == .enabled || FileManager.default.fileExists(atPath: agentURL.path)
    }

    static func set(_ on: Bool) throws {
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
