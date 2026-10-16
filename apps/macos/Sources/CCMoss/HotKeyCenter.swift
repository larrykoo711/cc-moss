import AppKit
import Carbon.HIToolbox
import MossKit
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

    /// "MOSS", so the event is recognizably ours.
    private static let signature = OSType(0x4D4F_5353)

    /// Register what Settings says, or the default.
    func start() {
        installHandler()
        _ = register(HotKey.load(.standard))
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

    func resume() { if let k = HotKey.load(.standard) { _ = attempt(k) } }

    private func attempt(_ key: HotKey) -> Bool {
        var newRef: EventHotKeyRef?
        let status = RegisterEventHotKey(UInt32(key.keyCode), Self.carbon(key.modifiers),
                                         EventHotKeyID(signature: Self.signature, id: 1),
                                         GetApplicationEventTarget(), 0, &newRef)
        guard status == noErr, let newRef else { return false }
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
            DispatchQueue.main.async { MainActor.assumeIsolated { HotKeyCenter.shared.onPress?() } }
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
    @State private var key: HotKey? = HotKey.load(.standard)
    @State private var recording = false
    @State private var problem: String?
    @State private var monitor: Any?
    @State private var hover = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
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
        if HotKeyCenter.shared.register(new) {
            HotKey.save(new, .standard)
            key = new
            problem = nil
        } else {
            problem = L.t("Another app uses it", "被别的 App 占用了")
            HotKeyCenter.shared.resume()
        }
    }
}
