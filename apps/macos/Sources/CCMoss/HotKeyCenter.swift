import AppKit
import ApplicationServices
import Carbon.HIToolbox
import MossKit
import Observation
import OSLog
import SwiftUI

/// The global shortcut that opens the panel. A key combination goes
/// through Carbon's hot keys (no permission needed); a modifier tapped twice
/// (double Control, the default) needs key events from other apps, so
/// Accessibility. Until that is allowed, ⌃⌥M works instead.
@MainActor
final class HotKeyCenter {
    static let shared = HotKeyCenter()

    var onPress: (() -> Void)?
    private var ref: EventHotKeyRef?
    private var handler: EventHandlerRef?
    private(set) var current: HotKey?
    private(set) var shortcut: PanelShortcut = .default

    fileprivate let log = Logger(subsystem: "io.github.larrykoo711.cc-moss", category: "hotkey")

    /// "MOSS", so the event is recognizably ours.
    private static let signature = OSType(0x4D4F_5353)

    /// The shortcut Settings says (double Control unless changed).
    func start() {
        installHandler()
        apply(PanelShortcut.load(.standard))
    }

    // MARK: - Permission and live keys

    @Observable
    final class Trust {
        var allowed = AXIsProcessTrusted()
    }
    let trust = Trust()

    /// The tapped modifier as you press it, for the welcome's keycaps.
    @Observable
    final class Keys {
        var down = false
        /// Presses so far (each release counts one).
        var presses = 0
    }
    let keys = Keys()
    private var monitors: [Any] = []
    private var tap = DoubleTap()
    private var trustTimer: Timer?

    /// Switch to `s`: stops the old way, starts the new one.
    func apply(_ s: PanelShortcut) {
        shortcut = s
        stopDoubleTap()
        unregister()
        switch s {
        case let .doubleTap(k):
            startDoubleTap(k)
            if !trust.allowed { _ = attempt(.default) }
        case let .combo(k):
            _ = attempt(k)
        case .none:
            break
        }
    }

    /// Off while you record a new one, so pressing the old one records it.
    func pause() {
        stopDoubleTap()
        unregister()
    }

    func resume() { apply(shortcut) }

    /// The system's own "allow Accessibility" prompt.
    func askForAccessibility() {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        if AXIsProcessTrustedWithOptions(options) { refreshTrust() }
    }

    static func openAccessibilitySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
            NSWorkspace.shared.open(url)
        }
    }

    static func flag(_ k: TapKey) -> NSEvent.ModifierFlags {
        switch k {
        case .control: .control
        case .option: .option
        case .shift: .shift
        case .command: .command
        }
    }

    private func startDoubleTap(_ k: TapKey) {
        let mine = Self.flag(k)
        let handle: (NSEvent) -> Void = { [weak self] event in
            MainActor.assumeIsolated {
                guard let self else { return }
                if event.type == .keyDown {
                    self.tap.key()
                    return
                }
                let f = event.modifierFlags.intersection([.command, .option, .shift, .control, .function, .capsLock])
                let others = !f.subtracting(mine).isEmpty
                let down = f.contains(mine) && !others
                if down != self.keys.down {
                    if !down { self.keys.presses += 1 }
                    self.keys.down = down
                }
                if self.tap.modifiers(key: f.contains(mine), others: others, at: event.timestamp) {
                    self.log.info("pressed (double \(k.rawValue, privacy: .public))")
                    self.onPress?()
                }
            }
        }
        if let m = NSEvent.addGlobalMonitorForEvents(matching: [.flagsChanged, .keyDown], handler: handle) { monitors.append(m) }
        if let m = NSEvent.addLocalMonitorForEvents(matching: [.flagsChanged, .keyDown], handler: { handle($0); return $0 }) {
            monitors.append(m)
        }
        // Watch for the permission arriving (or going away after an update).
        trustTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.refreshTrust() }
        }
    }

    private func stopDoubleTap() {
        monitors.forEach(NSEvent.removeMonitor)
        monitors.removeAll()
        trustTimer?.invalidate()
        trustTimer = nil
        tap = DoubleTap()
        keys.down = false
    }

    private func refreshTrust() {
        let now = AXIsProcessTrusted()
        guard now != trust.allowed else { return }
        trust.allowed = now
        log.info("accessibility \(now ? "allowed" : "not allowed", privacy: .public)")
        // Monitors added before the permission don't see other apps' keys: start again.
        if shortcut.tapKey != nil { apply(shortcut) }
    }

    // MARK: - Combinations (Carbon)

    private func attempt(_ key: HotKey) -> Bool {
        var newRef: EventHotKeyRef?
        let status = RegisterEventHotKey(UInt32(key.keyCode), Self.carbon(key.modifiers),
                                         EventHotKeyID(signature: Self.signature, id: 1),
                                         GetApplicationEventTarget(), 0, &newRef)
        guard status == noErr, let newRef else {
            log.error("could not register \(key.display, privacy: .public): \(status)")
            return false
        }
        log.info("registered \(key.display, privacy: .public)")
        ref = newRef
        current = key
        return true
    }

    private func unregister() {
        if let ref { UnregisterEventHotKey(ref) }
        ref = nil
        current = nil
    }

    private func installHandler() {
        guard handler == nil else { return }
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    HotKeyCenter.shared.log.info("pressed")
                    HotKeyCenter.shared.onPress?()
                }
            }
            return noErr
        }, 1, &spec, nil, &handler)
    }

    private static func carbon(_ m: HotKey.Modifiers) -> UInt32 {
        var out = 0
        if m.contains(.command) { out |= cmdKey }
        if m.contains(.option) { out |= optionKey }
        if m.contains(.control) { out |= controlKey }
        if m.contains(.shift) { out |= shiftKey }
        return UInt32(out)
    }

    static func modifiers(_ flags: NSEvent.ModifierFlags) -> HotKey.Modifiers {
        var m: HotKey.Modifiers = []
        if flags.contains(.command) { m.insert(.command) }
        if flags.contains(.option) { m.insert(.option) }
        if flags.contains(.control) { m.insert(.control) }
        if flags.contains(.shift) { m.insert(.shift) }
        return m
    }
}

/// Records the panel's shortcut, in Settings and in the welcome. Click it,
/// then either tap a modifier twice (⌃ ⌃, ⌥ ⌥…) or press a combination
/// (⌃⌥M). Esc cancels, Delete clears. Shows the keys as keycaps.
struct HotKeyRecorder: View {
    var onSave: ((PanelShortcut) -> Void)?
    /// The welcome's: a whole row, taller, with a line under it.
    var large = false

    @State private var value = PanelShortcut.load(.standard)
    @State private var recording = false
    @State private var problem: String?
    @State private var monitor: Any?
    @State private var hover = false
    @State private var taps: [TapKey: DoubleTap] = [:]
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button(action: toggle) {
                HStack(spacing: 8) {
                    if recording {
                        Image(systemName: "record.circle")
                            .font(.system(size: large ? 16 : 13))
                            .foregroundStyle(Theme.brand)
                            .symbolEffect(.pulse, isActive: true)
                        Text(L.t("Tap a key twice, or press keys…", "连按两下某个键，或按下组合键…"))
                            .font(.system(size: large ? 13 : 12))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    } else if value == .none {
                        Text(L.t("Click to set", "点这里设置")).font(.system(size: 12)).foregroundStyle(.secondary)
                    } else {
                        Keycaps(shortcut: value, large: large)
                        if value == .default {
                            Text(L.t("Default", "默认"))
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(Theme.brand)
                                .padding(.horizontal, 6).padding(.vertical, 2)
                                .background(Capsule().fill(Theme.brand.opacity(0.12)))
                        }
                    }
                    Spacer(minLength: 6)
                    if !recording {
                        Text(L.t("Change", "更换")).font(.system(size: 11)).foregroundStyle(.tertiary)
                    }
                }
                .padding(.horizontal, large ? 14 : 10)
                .frame(minWidth: large ? nil : 236, maxWidth: large ? .infinity : 236, minHeight: large ? 52 : 34)
                .background(RoundedRectangle(cornerRadius: large ? 12 : 9, style: .continuous)
                    .fill(recording ? Theme.brand.opacity(0.08) : (hover ? Theme.selection(scheme) : Color.primary.opacity(0.045))))
                .overlay(RoundedRectangle(cornerRadius: large ? 12 : 9, style: .continuous)
                    .strokeBorder(recording ? Theme.brand.opacity(0.7) : Color.primary.opacity(0.08), lineWidth: recording ? 1.5 : 1))
                .contentShape(Rectangle())
            }
            .buttonStyle(PressStyle())
            .focusable(false)
            .handCursor()
            .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hover = inside } }
            .animation(.spring(response: 0.3, dampingFraction: 0.8), value: recording)
            if large || problem != nil {
                Text(problem ?? (recording ? L.t("Esc cancels, Delete clears.", "Esc 取消，删除键清除。")
                                           : L.t("Click, then tap a key twice or press a combination.", "点一下，再连按两下某个键，或者按下组合键。")))
                    .font(.system(size: 11))
                    .foregroundStyle(problem == nil ? Theme.caption : Theme.amber)
                    .lineLimit(1)
            }
        }
        .onDisappear(perform: stop)
    }

    private func toggle() {
        recording ? stop() : start()
    }

    private func start() {
        problem = nil
        recording = true
        taps = [:]
        HotKeyCenter.shared.pause()
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { event in
            MainActor.assumeIsolated { handle(event) }
            return nil
        }
    }

    private func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        guard recording else { return }
        recording = false
        HotKeyCenter.shared.resume()
    }

    private func handle(_ event: NSEvent) {
        if event.type == .flagsChanged {
            let f = event.modifierFlags.intersection([.command, .option, .shift, .control])
            for k in TapKey.allCases {
                let mine = HotKeyCenter.flag(k)
                var t = taps[k] ?? DoubleTap()
                let fired = t.modifiers(key: f.contains(mine), others: !f.subtracting(mine).isEmpty, at: event.timestamp)
                taps[k] = t
                if fired {
                    save(.doubleTap(k))
                    return
                }
            }
            return
        }
        for k in TapKey.allCases { taps[k]?.key() }
        switch Int(event.keyCode) {
        case kVK_Escape:
            stop()
            return
        case kVK_Delete, kVK_ForwardDelete:
            save(.none)
            return
        default:
            break
        }
        let candidate = HotKey(keyCode: Int(event.keyCode), key: event.charactersIgnoringModifiers ?? "",
                               modifiers: HotKeyCenter.modifiers(event.modifierFlags))
        guard candidate.isValid else {
            problem = L.t("Add ⌃, ⌥ or ⌘, or tap one key twice", "要带上 ⌃、⌥ 或 ⌘，或者连按两下某个键")
            return
        }
        save(.combo(candidate))
    }

    private func save(_ next: PanelShortcut) {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        recording = false
        let center = HotKeyCenter.shared
        let previous = center.shortcut
        center.apply(next)
        if case let .combo(k) = next, center.current != k {
            problem = L.t("Another app uses it", "被别的 App 占用了")
            center.apply(previous)
            return
        }
        PanelShortcut.save(next, .standard)
        withAnimation(.spring(response: 0.3, dampingFraction: 0.75)) { value = next }
        problem = nil
        onSave?(next)
    }
}

/// A shortcut as keycaps: ⌃ ⌃ for a double tap, ⌃ ⌥ M for a combination.
struct Keycaps: View {
    let shortcut: PanelShortcut
    var large = false
    @Environment(\.colorScheme) private var scheme

    private var keys: [String] {
        switch shortcut {
        case let .doubleTap(k): [k.glyph, k.glyph]
        case let .combo(k):
            k.display.map(String.init).filter { $0 != " " }.reduce(into: [String]()) { out, c in
                if "⌃⌥⇧⌘".contains(c) || out.isEmpty || "⌃⌥⇧⌘".contains(out.last!) { out.append(c) } else { out[out.count - 1] += c }
            }
        case .none: []
        }
    }

    var body: some View {
        HStack(spacing: 4) {
            ForEach(Array(keys.enumerated()), id: \.offset) { _, k in
                Text(k)
                    .font(.system(size: large ? 14 : 12, weight: .medium))
                    .frame(minWidth: large ? 26 : 22, minHeight: large ? 26 : 22)
                    .padding(.horizontal, k.count > 1 ? 4 : 0)
                    .background(RoundedRectangle(cornerRadius: 5, style: .continuous).fill(Theme.raised(scheme)))
                    .overlay(RoundedRectangle(cornerRadius: 5, style: .continuous).strokeBorder(Theme.hairline(scheme)))
                    .shadow(color: .black.opacity(0.06), radius: 0.5, y: 1)
            }
            if case .doubleTap = shortcut {
                Text(L.t("twice", "连按两下"))
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                    .padding(.leading, 2)
            }
        }
    }
}

/// Under the shortcut row while a double tap still needs Accessibility.
struct AccessibilityRow: View {
    let shortcut: PanelShortcut

    var body: some View {
        let k = shortcut.tapKey?.glyph ?? "⌃"
        CardRow(title: L.t("Let cc-moss hear \(k)\(k)", "让 cc-moss 听见连按 \(k)"),
                detail: L.t("Turn it on once in System Settings. Until then, use ⌃⌥M.",
                            "在系统设置里打开一次就好，之前先用 ⌃⌥M。"),
                divider: false,
                icon: RowIcon("accessibility", .blue)) {
            SoftButton(title: L.t("Allow", "去允许"), icon: "hand.raised") {
                HotKeyCenter.shared.askForAccessibility()
                HotKeyCenter.openAccessibilitySettings()
            }
        }
    }
}
