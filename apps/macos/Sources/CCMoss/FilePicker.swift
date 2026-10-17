import AppKit
import MossKit

/// While something of ours sits over the menu bar panel (a dropdown list, the
/// system's folder picker), the panel stays open: losing the keyboard to it
/// isn't "you clicked elsewhere".
@MainActor
enum PanelHold {
    private static var depth = 0
    static var active: Bool { depth > 0 }

    static func begin() { depth += 1 }
    static func end() { depth = max(0, depth - 1) }
}

/// The system's file and folder picker, the same way everywhere: as a sheet
/// on the window you're in (Settings, the welcome), or, from the menu bar
/// panel, floating just above it while the panel and its form stay put.
@MainActor
enum FilePicker {
    enum Kind {
        case folder
        case program
    }

    static func choose(_ kind: Kind, from window: NSWindow?, startingAt start: String?,
                       message: String, prompt: String, done: @escaping (String) -> Void) {
        let panel = NSOpenPanel()
        panel.canChooseFiles = kind == .program
        panel.canChooseDirectories = kind == .folder
        panel.canCreateDirectories = kind == .folder
        panel.allowsMultipleSelection = false
        panel.showsHiddenFiles = kind == .program
        panel.message = message
        panel.prompt = prompt
        if let start, !start.isEmpty {
            panel.directoryURL = URL(fileURLWithPath: start, isDirectory: kind == .folder)
        }
        if let window, window.isVisible, !(window is KeyPanel) {
            panel.beginSheetModal(for: window) { response in
                if response == .OK, let url = panel.url { done(url.path) }
            }
            return
        }
        // The menu bar panel floats over normal windows: the picker goes one
        // level higher, or it would open behind the panel.
        panel.level = NSWindow.Level(rawValue: NSWindow.Level.statusBar.rawValue + 1)
        PanelHold.begin()
        NSApp.activate()
        let response = panel.runModal()
        PanelHold.end()
        window?.makeKeyAndOrderFront(nil)
        if response == .OK, let url = panel.url { done(url.path) }
    }
}

extension FolderChoice {
    /// A row for a folder list: its name, where it lives, a symbol.
    static func option(_ path: String, symbol: String, badge: String? = nil) -> DropdownOption<String> {
        let label = label(path, home: NSHomeDirectory())
        return DropdownOption(id: path, title: label.name, subtitle: label.parent, symbol: symbol, badge: badge)
    }

    /// "Use ~/code": what you typed, when it's a folder.
    static func typedOption(_ query: String) -> DropdownOption<String>? {
        let path = typedPath(query, home: NSHomeDirectory()) { p in
            var dir: ObjCBool = false
            return FileManager.default.fileExists(atPath: p, isDirectory: &dir) && dir.boolValue
        }
        return path.map { option($0, symbol: "arrow.turn.down.right", badge: L.t("Use", "用这个")) }
    }
}
