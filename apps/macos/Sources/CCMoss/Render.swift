import AppKit
import MossKit
import SwiftUI

/// Off-screen rendering of the panel for screenshots.
///
///   CCMoss --render out.png --demo sessions.json [--dark] [--query q]
///          [--filter working|waiting|interrupted] [--select <id>] [--actions]
///          [--now 2026-10-16T12:00:00Z]
///          [--terminals terminal,ghostty] [--problem denied|failed]
///          [--settings general|aliases|about [--aliases <alias ls --json file>]]
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
        if let state = value("--icon") {
            icon(state, args: args, out: out)
            return
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
                // `--aliases <alias ls --json file>` for the aliases page.
                let aliasDemo = value("--aliases").flatMap { FileManager.default.contents(atPath: $0) }
                    .flatMap { try? AliasList.decode($0) }
                let page: SettingsView.Tab = switch tab {
                case "about": .about
                case "aliases": .aliases
                default: .general
                }
                drawSettings(model: model, tab: page, aliasDemo: aliasDemo, dark: args.contains("--dark"), out: out)
            } else {
                draw(model: model, dark: args.contains("--dark"), actions: args.contains("--actions"), out: out)
            }
            done.signal()
            exit(0)
        }
        RunLoop.main.run()
    }

    /// The menu bar icon on a stand-in menu bar, for reviewing its frames.
    ///
    ///   CCMoss --render out.png --icon working|waiting|needsYou|interrupted|idle|all
    ///          [--frame n | --sheet] [--count n] [--dark] [--scale 1|2]
    ///
    /// `--sheet` lays out every frame of the state left to right; `all` shows
    /// each state's first frame. Template states are drawn in the bar's text colour.
    @MainActor static func icon(_ name: String, args: [String], out: String) {
        func value(_ flag: String) -> String? {
            guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        let states: [String: IconState] = [
            "working": .working, "waiting": .waiting, "needsYou": .needsYou,
            "interrupted": .interrupted, "idle": .idle,
        ]
        let frames: [IconFrame]
        if name == "all" {
            frames = [.needsYou, .interrupted, .working, .waiting, .idle].map { IconFrame(state: $0, index: 0) }
        } else if let state = states[name] {
            if args.contains("--sheet") {
                frames = (0..<IconMotion.frameCount(state)).map { IconFrame(state: state, index: $0) }
            } else {
                let n = Int(value("--frame") ?? "0") ?? 0
                frames = [IconFrame(state: state, index: n % IconMotion.frameCount(state))]
            }
        } else {
            FileHandle.standardError.write(Data("--icon: one of \(states.keys.sorted().joined(separator: ", ")), all\n".utf8))
            exit(2)
        }
        let badge = value("--count").flatMap(Int.init)
        let dark = args.contains("--dark")
        let scale = CGFloat(Double(value("--scale") ?? "2") ?? 2)
        let cell = StatusIcon.size(badge: badge)
        let pad: CGFloat = 8
        let bar: CGFloat = 24
        // `--compare`: system menu bar symbols to the left, to judge weight.
        let neighbours = args.contains("--compare") ? ["wifi", "magnifyingglass", "lock.fill"] : []
        let neighbourWidth: CGFloat = 22
        let lead = CGFloat(neighbours.count) * neighbourWidth
        let width = lead + pad + CGFloat(frames.count) * (cell.width + pad)
        let pixels = NSSize(width: (width * scale).rounded(), height: (bar * scale).rounded())
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(pixels.width), pixelsHigh: Int(pixels.height),
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ), let gc = NSGraphicsContext(bitmapImageRep: rep) else { exit(1) }
        let ctx = gc.cgContext
        // y-down, in points.
        ctx.translateBy(x: 0, y: pixels.height)
        ctx.scaleBy(x: scale, y: -scale)
        let background = dark ? NSColor(red: 0.16, green: 0.16, blue: 0.18, alpha: 1) : NSColor(red: 0.93, green: 0.93, blue: 0.94, alpha: 1)
        ctx.setFillColor(background.cgColor)
        ctx.fill(CGRect(x: 0, y: 0, width: width, height: bar))
        let ink = dark ? NSColor.white : NSColor.black.withAlphaComponent(0.85)
        let symbolConfig = NSImage.SymbolConfiguration(pointSize: 14, weight: .regular)
        for (i, name) in neighbours.enumerated() {
            guard let symbol = NSImage(systemSymbolName: name, accessibilityDescription: nil)?
                .withSymbolConfiguration(symbolConfig) else { continue }
            let size = symbol.size
            let r = NSRect(x: pad + CGFloat(i) * neighbourWidth + (neighbourWidth - size.width) / 2 - 4,
                           y: (bar - size.height) / 2, width: size.width, height: size.height)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
            let tinted = NSImage(size: size, flipped: false) { rect in
                symbol.draw(in: rect)
                ink.set()
                rect.fill(using: .sourceAtop)
                return true
            }
            tinted.draw(in: r, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
            NSGraphicsContext.restoreGraphicsState()
        }
        for (i, frame) in frames.enumerated() {
            let x = lead + pad + CGFloat(i) * (cell.width + pad)
            StatusIcon.draw(frame, badge: badge, in: ctx, rect: CGRect(x: x, y: 0, width: cell.width, height: bar), ink: ink)
        }
        ctx.flush()
        guard let png = rep.representation(using: .png, properties: [:]) else { exit(1) }
        do {
            try png.write(to: URL(fileURLWithPath: out))
        } catch {
            FileHandle.standardError.write(Data("\(error)\n".utf8))
            exit(1)
        }
        exit(0)
    }

    /// `CCMoss --bench-icon working|needsYou|idle [--seconds n]`: run the icon's
    /// real timer into an off-screen button and print this process's CPU use.
    /// It covers our side of a frame (timer, cache, setting the image), not
    /// the menu bar's own redraw.
    @MainActor static func benchIcon(args: [String]) {
        func value(_ flag: String) -> String? {
            guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        let seconds = Double(value("--seconds") ?? "10") ?? 10
        let state = value("--bench-icon") ?? "working"
        let session: String = switch state {
        case "needsYou": #"{"status":"running","busy":true,"attention":"permission"}"#
        case "idle": #"{"status":"closed","busy":false}"#
        default: #"{"status":"running","busy":true}"#
        }
        let json = #"{"schema":1,"sessions":[{"id":"A","session_id":"s","cwd":"/w","last_active_at":"2026-10-16T12:00:00Z","notes":{}, "#
            + session.dropFirst() + "]}"
        guard let snap = try? Snapshot.decode(Data(json.utf8)) else { exit(1) }
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 60, height: 24), styleMask: [.borderless], backing: .buffered, defer: false)
        let button = NSButton(frame: window.contentLayoutRect)
        window.contentView = button
        let icon = MenuBarIcon()
        var frames = 0
        icon.onImage = { image in
            button.image = image
            button.display()
            frames += 1
        }
        icon.update(snap.sessions, showCount: false)
        func cpu() -> Double {
            var usage = rusage()
            getrusage(RUSAGE_SELF, &usage)
            return Double(usage.ru_utime.tv_sec + usage.ru_stime.tv_sec)
                + Double(usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) / 1_000_000
        }
        // Let the first frames be drawn and cached before measuring.
        RunLoop.main.run(until: Date().addingTimeInterval(2))
        let start = cpu(), began = Date(), startFrames = frames
        RunLoop.main.run(until: Date().addingTimeInterval(seconds))
        let wall = Date().timeIntervalSince(began)
        let used = cpu() - start
        print(String(format: "%@: %.2f%% CPU over %.1f s, %d frames (%.1f fps)",
                     state, used / wall * 100, wall, frames - startFrames, Double(frames - startFrames) / wall))
        exit(0)
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

    @MainActor private static func drawSettings(model: AppModel, tab: SettingsView.Tab, aliasDemo: AliasList?, dark: Bool, out: String) {
        let root = SettingsView(model: model, tab: tab, aliasDemo: aliasDemo)
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
