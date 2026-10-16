import AppKit
import MossKit
import SwiftUI

/// Off-screen rendering of the panel for screenshots.
///
///   CCMoss --render out.png --demo sessions.json [--dark] [--query q]
///          [--filter working|waiting|interrupted] [--select <id>] [--actions]
///          [--now 2026-10-16T12:00:00Z]
///          [--terminals terminal,ghostty] [--problem denied|failed]
enum Render {
    /// `--terminals a,b`: pretend only these are installed (real icons);
    /// the choice is never saved.
    @MainActor static func terminals(_ args: [String]) -> TerminalPrefs {
        let defaults = UserDefaults(suiteName: "cc-moss-render") ?? .standard
        defaults.removeObject(forKey: "terminal")
        guard let i = args.firstIndex(of: "--terminals"), i + 1 < args.count else {
            return TerminalPrefs(defaults: defaults)
        }
        let list = args[i + 1].split(separator: ",").compactMap { Terminal(rawValue: String($0)) }
        return TerminalPrefs(catalog: .only(list), defaults: defaults)
    }

    @MainActor static func run(out: String, args: [String]) {
        func value(_ flag: String) -> String? {
            guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        guard let demo = value("--demo"), let data = FileManager.default.contents(atPath: demo) else {
            FileHandle.standardError.write(Data("--render needs --demo <ls --json file>\n".utf8))
            exit(2)
        }
        if let now = value("--now"), let date = ISO8601DateFormatter().date(from: now) {
            L.clock = { date }
        }
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let model = AppModel(client: MossClient(executable: "/demo/cc-moss", runner: CannedRunner(stdout: data)), terminals: terminals(args))
        let done = DispatchSemaphore(value: 0)
        Task { @MainActor in
            await model.loadNow()
            model.query = value("--query") ?? ""
            switch value("--filter") {
            case "working": model.stateFilter = .working
            case "waiting": model.stateFilter = .waiting
            case "interrupted": model.stateFilter = .interrupted
            default: break
            }
            if let id = value("--select") { model.selectedID = id }
            let command = model.selected.map(model.attachCommand) ?? ""
            switch value("--problem") {
            case "denied": model.openProblem = OpenProblem(kind: .denied, terminalName: "Ghostty", command: command)
            case "failed": model.openProblem = OpenProblem(kind: .failed("Connection is invalid (-609)"), terminalName: "Ghostty", command: command)
            default: break
            }
            if let tab = value("--settings") {
                drawSettings(model: model, tab: tab == "about" ? .about : .general, dark: args.contains("--dark"), out: out)
            } else {
                draw(model: model, dark: args.contains("--dark"), actions: args.contains("--actions"), out: out)
            }
            done.signal()
            exit(0)
        }
        RunLoop.main.run()
    }

    @MainActor static func preview(args: [String]) {
        func value(_ flag: String) -> String? {
            guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        guard let demo = value("--demo"), let data = FileManager.default.contents(atPath: demo) else { exit(2) }
        if let now = value("--now"), let date = ISO8601DateFormatter().date(from: now) { L.clock = { date } }
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let model = AppModel(client: MossClient(executable: "/demo/cc-moss", runner: CannedRunner(stdout: data)), terminals: terminals(args))
        let settings = args.contains("--settings-window")
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: settings ? 820 : Theme.panelWidth, height: settings ? 560 : Theme.panelHeight),
            styleMask: settings ? [.titled, .closable, .fullSizeContentView] : [.titled, .closable],
            backing: .buffered, defer: false
        )
        window.title = "cc-moss preview"
        if settings {
            window.titlebarAppearsTransparent = true
            window.titleVisibility = .hidden
            window.contentView = NSHostingView(rootView: SettingsView(model: model))
        } else {
            window.contentView = NSHostingView(rootView: PanelView(model: model))
        }
        // Top-left corner of the main screen, so recordings know where to look.
        if let screen = NSScreen.main {
            window.setFrameTopLeftPoint(NSPoint(x: screen.frame.minX + 40, y: screen.frame.maxY - 60))
        }
        window.makeKeyAndOrderFront(nil)
        if settings { SettingsWindow.tuckWindowButtons(in: window) }
        app.activate(ignoringOtherApps: true)
        // `--cycle`: step through the state filter every 1.5 s, hands-free.
        if args.contains("--cycle") {
            Timer.scheduledTimer(withTimeInterval: 1.5, repeats: true) { _ in
                Task { @MainActor in
                    model.stateFilter = StateFilterBar.cycle(model.stateFilter, forward: true)
                }
            }
        }
        app.run()
    }

    @MainActor private static func drawSettings(model: AppModel, tab: SettingsView.Tab, dark: Bool, out: String) {
        let root = SettingsView(model: model, tab: tab)
            .environment(\.colorScheme, dark ? .dark : .light)
        let host = NSHostingView(rootView: root)
        let size = host.fittingSize
        host.frame = NSRect(origin: .zero, size: size)
        let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.8))
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { exit(1) }
        host.cacheDisplay(in: host.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: out))
    }

    @MainActor private static func draw(model: AppModel, dark: Bool, actions: Bool, out: String) {
        let root = PanelView(model: model, showActions: actions)
            // Stand-in for the desktop behind the frosted glass.
            .background(dark ? Color(red: 0.16, green: 0.22, blue: 0.30) : Color(red: 0.55, green: 0.66, blue: 0.75))
            .environment(\.colorScheme, dark ? .dark : .light)
        let host = NSHostingView(rootView: root)
        host.frame = NSRect(x: 0, y: 0, width: Theme.panelWidth, height: Theme.panelHeight)
        let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        // Let SwiftUI settle (fonts, lazy rows) before taking the picture.
        RunLoop.main.run(until: Date().addingTimeInterval(0.6))
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { exit(1) }
        host.cacheDisplay(in: host.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else { exit(1) }
        try? png.write(to: URL(fileURLWithPath: out))
    }
}
