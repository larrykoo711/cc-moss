import AppKit
import ApplicationServices
import Carbon.HIToolbox
import MossKit
import Observation
import OSLog
import SwiftUI

/// The global shortcut that opens the panel: Carbon's hot keys, which need
/// no Accessibility permission (unlike a global key monitor).
@MainActor
final class HotKeyCenter {
    static let shared = HotKeyCenter()

    var onPress: (() -> Void)?
    private var ref: EventHotKeyRef?
    private var handler: EventHandlerRef?
    private(set) var current: HotKey?

    private let log = Logger(subsystem: "io.github.larrykoo711.cc-moss", category: "hotkey")

    /// "MOSS", so the event is recognizably ours.
    private static let signature = OSType(0x4D4F_5353)

    /// The way Settings says (double Control unless changed).
    func start() {
        installHandler()
        apply(PanelShortcut.load(.standard))
    }

    // MARK: - Double Control

    /// Double Control needs Accessibility (key events from other apps);
    /// until it is allowed, the ⌃⌥M combination works instead.
    @Observable
    final class Trust {
        var allowed = AXIsProcessTrusted()
    }
    let trust = Trust()

    /// Control as you press it, for the welcome's keycaps.
    @Observable
    final class Keys {
        var controlDown = false
        /// Control presses so far (each release counts one).
        var presses = 0
    }
    let keys = Keys()
    private var monitors: [Any] = []
    private var tap = DoubleTap()
    private var trustTimer: Timer?
    private(set) var shortcut: PanelShortcut = .doubleControl

    /// Switch to `s`: stops the old way, starts the new one.
    func apply(_ s: PanelShortcut) {
        shortcut = s
        stopDoubleControl()
        unregister()
        switch s {
        case .doubleControl:
            startDoubleControl()
            if !trust.allowed { _ = attempt(.default) }
        case let .combo(k):
            _ = attempt(k)
        case .none:
            break
        }
    }

    /// The system's own "allow Accessibility" prompt, then System Settings.
    func askForAccessibility() {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        if AXIsProcessTrustedWithOptions(options) { refreshTrust() }
    }

    static func openAccessibilitySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
            NSWorkspace.shared.open(url)
        }
    }

    private func startDoubleControl() {
        let handle: (NSEvent) -> Void = { [weak self] event in
            MainActor.assumeIsolated {
                guard let self else { return }
                if event.type == .keyDown {
                    self.tap.key()
                    return
                }
                let f = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
                let others = !f.intersection([.command, .option, .shift, .function, .capsLock]).isEmpty
                let down = f.contains(.control) && !others
                if down != self.keys.controlDown {
                    if !down { self.keys.presses += 1 }
                    self.keys.controlDown = down
                }
                if self.tap.modifiers(control: f.contains(.control), others: others, at: event.timestamp) {
                    self.log.info("pressed (double control)")
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

    private func stopDoubleControl() {
        monitors.forEach(NSEvent.removeMonitor)
        monitors.removeAll()
        trustTimer?.invalidate()
        trustTimer = nil
        tap = DoubleTap()
    }

    private func refreshTrust() {
        let now = AXIsProcessTrusted()
        guard now != trust.allowed else { return }
        trust.allowed = now
        log.info("accessibility \(now ? "allowed" : "not allowed", privacy: .public)")
        // Monitors added before the permission don't see other apps' keys: start again.
        if shortcut == .doubleControl { apply(.doubleControl) }
    }

    /// False when the system refuses it (another app already uses it); the
    /// previous one stays registered then.
    @discardableResult
    func register(_ key: HotKey?) -> Bool {
        let previous = current
        unregister()
        guard let key else { return true }
        if attempt(key) { return true }
        if let previous { _ = attempt(previous) }
        return false
    }

    /// Off while you record a new one, so pressing the old one records it.
    func pause() { unregister() }

    func resume() { apply(shortcut) }

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

/// Settings › General: shows the shortcut; click to record a new one.
/// While recording: press the combination, Esc cancels, Delete clears.
struct HotKeyRecorder: View {
    var onSave: ((PanelShortcut) -> Void)?
    /// The welcome's version: a whole row, large type, a breathing border while recording.
    var large = false
    @State private var key: HotKey? = {
        if case let .combo(k) = PanelShortcut.load(.standard) { return k }
        return nil
    }()
    @State private var recording = false
    @State private var problem: String?
    @State private var monitor: Any?
    @State private var hover = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        if large { largeBody } else { smallBody }
    }

    /// A whole row: the combination in large type, or "press keys" with a
    /// breathing brand border while recording; a line under it says what's next.
    private var largeBody: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button(action: toggle) {
                HStack(spacing: 10) {
                    Image(systemName: recording ? "record.circle" : "keyboard")
                        .font(.system(size: 16))
                        .foregroundStyle(recording ? Theme.brand : Color.secondary)
                        .symbolEffect(.pulse, isActive: recording)
                    Text(recording ? L.t("Press the keys you want…", "按下你想用的组合键…") : (key?.display ?? L.t("Click to set", "点这里设置")))
                        .font(.system(size: recording || key == nil ? 14 : 18, weight: key == nil || recording ? .regular : .semibold))
                        .foregroundStyle(recording || key == nil ? .secondary : .primary)
                    Spacer()
                    if !recording, key != nil {
                        Text(L.t("Change", "更换")).font(.system(size: 12)).foregroundStyle(.secondary)
                    }
                }
                .padding(.horizontal, 14)
                .frame(maxWidth: .infinity, minHeight: 52)
                .background(RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(recording ? Theme.brand.opacity(0.08) : (hover ? Theme.selection(scheme) : Color.primary.opacity(0.04))))
                .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(recording ? Theme.brand.opacity(0.7) : Color.primary.opacity(0.1),
                                  style: StrokeStyle(lineWidth: recording ? 1.5 : 1, dash: recording ? [] : [4, 3])))
                .contentShape(Rectangle())
            }
            .buttonStyle(PressStyle())
            .focusable(false)
            .handCursor()
            .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hover = inside } }
            .animation(.spring(response: 0.3, dampingFraction: 0.8), value: recording)
            Text(problem ?? (recording ? L.t("Include ⌃, ⌥ or ⌘. Esc cancels.", "要带上 ⌃、⌥ 或 ⌘；按 Esc 取消。")
                                       : L.t("Click, then press the keys together.", "点一下，再同时按下几个键。")))
                .font(.system(size: 11))
                .foregroundStyle(problem == nil ? Theme.caption : Theme.amber)
        }
        .onDisappear(perform: stop)
    }

    private var smallBody: some View {
        HStack(spacing: 8) {
            if let problem {
                Text(problem)
                    .font(.system(size: 12))
                    .foregroundStyle(Theme.amber)
            }
            Button(action: toggle) {
                Text(label)
                    .font(.system(size: 13, weight: key == nil || recording ? .regular : .medium))
                    .foregroundStyle(recording || key == nil ? .secondary : .primary)
                    .frame(minWidth: 96)
                    .padding(.horizontal, 10)
                    .frame(height: 26)
                    .background(RoundedRectangle(cornerRadius: 7, style: .continuous)
                        .fill(recording ? Theme.brand.opacity(0.12) : (hover ? Theme.selection(scheme) : Color.primary.opacity(0.06))))
                    .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous)
                        .strokeBorder(recording ? Theme.brand.opacity(0.6) : .clear))
                    .contentShape(Rectangle())
            }
            .buttonStyle(PressStyle())
            .focusable(false)
            .handCursor()
            .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hover = inside } }
            .tip(recording ? L.t("Esc to cancel, Delete to clear", "Esc 取消，删除键清除")
                           : L.t("Click, then press the keys", "点一下，再按下组合键"))
        }
        .onDisappear(perform: stop)
    }

    private var label: String {
        if recording { return L.t("Press keys…", "按下组合键…") }
        return key?.display ?? L.t("None", "未设置")
    }

    private func toggle() {
        recording ? stop() : start()
    }

    private func start() {
        problem = nil
        recording = true
        HotKeyCenter.shared.pause()
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
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
        switch Int(event.keyCode) {
        case kVK_Escape:
            stop()
            return
        case kVK_Delete, kVK_ForwardDelete:
            save(nil)
            return
        default:
            break
        }
        let candidate = HotKey(keyCode: Int(event.keyCode), key: event.charactersIgnoringModifiers ?? "",
                               modifiers: HotKeyCenter.modifiers(event.modifierFlags))
        guard candidate.isValid else {
            problem = L.t("Add ⌃, ⌥ or ⌘", "要带上 ⌃、⌥ 或 ⌘")
            return
        }
        save(candidate)
    }

    private func save(_ new: HotKey?) {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        recording = false
        let center = HotKeyCenter.shared
        let previous = center.shortcut
        let next: PanelShortcut = new.map(PanelShortcut.combo) ?? .none
        center.apply(next)
        if let new, center.current != new {
            problem = L.t("Another app uses it", "被别的 App 占用了")
            center.apply(previous)
            return
        }
        PanelShortcut.save(next, .standard)
        key = new
        problem = nil
        onSave?(next)
    }
}

/// Settings › General and the welcome: how to call the panel. Double
/// Control (recommended) or a combination; double Control explains and asks
/// for Accessibility when it doesn't have it yet.
struct PanelShortcutPicker: View {
    @Binding var mode: String

    var body: some View {
        HStack(spacing: 8) {
                SlidingPicker(selection: $mode, options: [
                    ("double", L.t("Tap ⌃ Twice", "连按两下 ⌃")),
                    ("combo", L.t("Key Combo", "组合键")),
                ])
                .onChange(of: mode) { _, new in
                    if new == "double" {
                        PanelShortcut.save(.doubleControl, .standard)
                        HotKeyCenter.shared.apply(.doubleControl)
                    } else {
                        let combo = HotKey.load(.standard) ?? .default
                        PanelShortcut.save(.combo(combo), .standard)
                        HotKeyCenter.shared.apply(.combo(combo))
                    }
                }
            if mode == "combo" { HotKeyRecorder() }
        }
    }

    static func currentMode() -> String {
        PanelShortcut.load(.standard) == .doubleControl ? "double" : "combo"
    }
}

/// Under the shortcut row while double Control still needs Accessibility.
struct AccessibilityRow: View {
    var body: some View {
        CardRow(title: L.t("Let cc-moss hear ⌃", "允许 cc-moss 听见 ⌃"),
                detail: L.t("One click in System Settings › Accessibility. Until then, ⌃⌥M works.",
                            "在「系统设置 › 辅助功能」里点一下就好；允许之前先用 ⌃⌥M。"),
                divider: false) {
            SoftButton(title: L.t("Allow", "去允许"), icon: "hand.raised") {
                HotKeyCenter.shared.askForAccessibility()
                HotKeyCenter.openAccessibilitySettings()
            }
        }
    }
}
