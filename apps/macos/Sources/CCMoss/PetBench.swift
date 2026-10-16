import AppKit
import MossKit

/// Measuring and probing Mo without the real app.
///
///   CCMoss --bench-pet idle|working|needsYou|interrupted|sleeping [--seconds n] [--baseline]
///
/// Runs the real `PetController` (timers, loops, layers) in a window placed
/// far off screen, with demo sessions, and prints this process's CPU use and
/// memory footprint. `--baseline` runs the same without Mo, to subtract. It
/// cannot see the render server's share (WindowServer): that, and anything
/// that depends on Mo being on a real screen, is measured on the machine.
///
///   CCMoss --pet-probe [alpha|toggle]
///
/// Puts a plain window and Mo over it on the real screen and logs which of
/// the two each click lands in: click the transparent corners round Mo.
/// For a person at the machine; nothing here is automatic.
enum PetBench {
    private static func value(_ args: [String], _ flag: String) -> String? {
        guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
        return args[i + 1]
    }

    static func demoSessions(_ state: String) -> Data {
        let session: String = switch state {
        case "needsYou": #""status":"running","busy":true,"attention":"permission""#
        case "interrupted": #""status":"interrupted","busy":false"#
        case "working": #""status":"running","busy":true"#
        default: #""status":"running","busy":false"#
        }
        let json = #"{"schema":1,"sessions":[{"id":"A","session_id":"s","cwd":"/w","last_active_at":"2026-10-16T12:00:00Z","notes":{},"#
            + session + "}]}"
        return Data(json.utf8)
    }

    /// Memory this process is charged for (what Activity Monitor shows).
    static func footprint() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return kr == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
    }

    static func cpu() -> Double {
        var usage = rusage()
        getrusage(RUSAGE_SELF, &usage)
        return Double(usage.ru_utime.tv_sec + usage.ru_stime.tv_sec)
            + Double(usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) / 1_000_000
    }

    @MainActor static func run(args: [String]) {
        let state = value(args, "--bench-pet") ?? "working"
        let seconds = Double(value(args, "--seconds") ?? "20") ?? 20
        let baseline = args.contains("--baseline")
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        // Never the user's settings: a throwaway defaults domain.
        let suite = "cc-moss-pet-bench"
        let defaults = UserDefaults(suiteName: suite) ?? .standard
        defaults.removePersistentDomain(forName: suite)
        defaults.set(!baseline, forKey: PetController.enabledKey)
        defaults.set(Double(value(args, "--size") ?? "64") ?? 64, forKey: PetController.sizeKey)
        if state == "sleeping" {
            defaults.set(Date().addingTimeInterval(3600).timeIntervalSince1970, forKey: PetController.doNotDisturbKey)
        }
        let model = AppModel(client: MossClient(executable: "/demo/cc-moss", runner: CannedRunner(stdout: demoSessions(state))))
        let before = footprint()
        var pet: PetController?
        Task { @MainActor in
            await model.loadNow()
            pet = PetController(model: model, showPanel: {}, defaults: defaults, offScreen: true)
        }
        // Let the first pose, its one-off moves and the layer tree settle.
        RunLoop.main.run(until: Date().addingTimeInterval(3))
        let after = footprint()
        let start = cpu(), began = Date()
        RunLoop.main.run(until: Date().addingTimeInterval(seconds))
        let wall = Date().timeIntervalSince(began)
        let used = cpu() - start
        print(String(format: "%@%@: %.2f%% CPU over %.1f s; footprint %.1f MB (%+.1f MB during setup); %@",
                     state, baseline ? " (baseline, no Mo)" : "", used / wall * 100, wall, after, after - before,
                     pet?.activity ?? "-"))
        defaults.removePersistentDomain(forName: suite)
        exit(0)
    }

    // MARK: - Click-through probe

    @MainActor private static var keep: [AnyObject] = []

    @MainActor static func probe(args: [String]) {
        let mode = value(args, "--pet-probe").flatMap(PetClickThrough.init) ?? .alpha
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        guard let screen = NSScreen.main else { exit(1) }
        let below = NSWindow(contentRect: NSRect(x: screen.visibleFrame.midX - 200, y: screen.visibleFrame.midY - 150, width: 400, height: 300),
                             styleMask: [.titled, .closable], backing: .buffered, defer: false)
        below.title = "cc-moss pet probe (\(mode.rawValue)): click round Mo"
        let target = ClickLog(frame: below.contentLayoutRect, name: "BELOW")
        below.contentView = target
        below.makeKeyAndOrderFront(nil)
        let side = PetView.side(body: 64)
        let panel = PetPanel(side: side)
        let pet = PetView(body: 64)
        pet.onClick = { n in print("MO click (count \(n))") }
        pet.onHover = { inside in print("MO hover \(inside)") }
        panel.contentView = pet
        panel.setFrameOrigin(NSPoint(x: below.frame.midX - side / 2, y: below.frame.midY - side / 2))
        panel.orderFrontRegardless()
        if mode == .toggle { keep.append(PetMouseGate(window: panel, view: pet)) }
        keep += [below, panel, pet]
        print("probe: \(mode.rawValue). Click the empty corners of Mo's square, then Mo itself. Close the window to quit.")
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: below, queue: .main) { _ in exit(0) }
        app.activate(ignoringOtherApps: true)
        app.run()
    }
}

/// Logs clicks for the probe.
private final class ClickLog: NSView {
    let name: String
    init(frame: NSRect, name: String) {
        self.name = name
        super.init(frame: frame)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    override func mouseDown(with event: NSEvent) {
        print("\(name) click at \(Int(event.locationInWindow.x)),\(Int(event.locationInWindow.y))")
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemTeal.withAlphaComponent(0.25).setFill()
        dirtyRect.fill()
    }
}
