import AppKit
import MossKit
import SwiftUI

/// A Latin name inside Chinese text gets a space on each side ("在 Ghostty 里"),
/// a Chinese one doesn't ("在终端里").
func zhPad(_ name: String, leading: Bool = true, trailing: Bool = true) -> String {
    guard name.first?.isASCII == true else { return name }
    return (leading ? " " : "") + name + (trailing ? " " : "")
}

/// An action's icon: the terminal's own app icon when it has one, else its symbol.
struct ActionIcon: View {
    let action: PanelAction
    var symbolSize: CGFloat = 12

    var body: some View {
        Group {
            if let image = action.image {
                Image(nsImage: image).resizable().interpolation(.high).frame(width: 16, height: 16)
            } else {
                Image(systemName: action.icon).font(.system(size: symbolSize))
            }
        }
        .frame(width: 16)
    }
}

/// Shown over the panel when a session didn't open: why, and what to do.
struct OpenProblemCard: View {
    let problem: OpenProblem
    let copy: (String) -> Void
    let dismiss: () -> Void
    @Environment(\.colorScheme) private var scheme

    private var symbol: String {
        problem.kind == .denied ? Icons.locked : Icons.warning
    }

    private var title: String {
        let name = problem.terminalName
        switch problem.kind {
        case .denied: return L.t("cc-moss isn't allowed to use \(name) yet", "cc-moss 还没获准操控\(zhPad(name, trailing: false))")
        case .failed: return L.t("\(name) didn't open", "\(zhPad(name, leading: false))没能打开")
        case .unsafeName: return L.t("This name is too tricky to type for you", "这个名字不方便替你敲")
        case .noTerminal: return L.t("No terminal found", "没找到终端")
        }
    }

    private var message: String {
        let name = problem.terminalName
        switch problem.kind {
        case .denied:
            return L.t("Switch on \(name) for cc-moss in Privacy & Security › Automation. Or copy the command and paste it yourself.",
                       "到「隐私与安全性 › 自动化」里给 cc-moss 打开\(zhPad(name, trailing: false))。也可以复制命令，自己贴进终端。")
        case .failed(let why):
            return L.t("It said: \(why). Copy the command and paste it into any terminal.",
                       "它说：\(why)。复制命令，贴进任意终端也一样。")
        case .unsafeName:
            return L.t("It has invisible control characters a terminal could take for keystrokes. Copy the command instead.",
                       "名字里有看不见的控制字符，终端可能把它当成按键。复制命令自己贴吧。")
        case .noTerminal:
            return L.t("Copy the command and paste it into any terminal.", "复制命令，贴进任意终端就行。")
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: symbol)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(Theme.amber)
                    .frame(width: 20)
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(.system(size: 13, weight: .semibold))
                    Text(message)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
                Button(action: dismiss) {
                    Image(systemName: "xmark")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 20, height: 20)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .tip(L.t("Dismiss (Esc)", "关闭（Esc）"))
                .accessibilityLabel(L.t("Dismiss", "关闭"))
            }
            HStack(spacing: 8) {
                Spacer()
                if problem.kind == .denied {
                    SoftButton(title: L.t("Open Automation Settings", "打开自动化设置"), icon: Icons.settings) {
                        NSWorkspace.shared.open(Opener.automationSettings)
                    }
                }
                SoftButton(title: L.t("Copy Command", "复制命令"), icon: Icons.copy, prominent: true) {
                    copy(problem.command)
                    dismiss()
                }
            }
        }
        .padding(12)
        .background(Theme.raised(scheme), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.hairline(scheme)))
        .shadow(color: .black.opacity(0.18), radius: 14, y: 6)
    }
}

/// Settings › General › Terminal: where sessions open, and a word for Ghostty.
struct TerminalSettings: View {
    let prefs: TerminalPrefs

    private var detail: String {
        if let gone = prefs.notice {
            return L.t("\(gone.displayName) isn't installed anymore, so sessions open in Terminal.",
                       "\(zhPad(gone.displayName, leading: false))已经不在这台 Mac 上了，会话改在终端里打开。")
        }
        return L.t("A new window, straight into the session. Another terminal? ⇧⌘C copies the command.",
                   "新开一个窗口，直接进到会话里。用别的终端？⇧⌘C 复制命令。")
    }

    var body: some View {
        SettingsCard(title: L.t("Terminal", "终端")) {
            CardRow(title: L.t("Open Sessions In", "会话在哪儿打开"), detail: detail, divider: prefs.ghosttyMissing) {
                TerminalPicker(prefs: prefs)
            }
            if prefs.ghosttyMissing, let url = Terminal.ghostty.downloadURL {
                CardRow(
                    title: L.t("Try Ghostty", "试试 Ghostty"),
                    detail: L.t("Fast, native, scriptable, and free.", "快、原生、能被脚本调动，还免费。"),
                    divider: false
                ) {
                    HStack(spacing: 8) {
                        RecommendedBadge()
                        SoftButton(title: L.t("Download", "下载"), icon: Icons.download) {
                            NSWorkspace.shared.open(url)
                        }
                    }
                }
            }
        }
        .onAppear { prefs.settle() }
        .onDisappear { prefs.notice = nil }
    }
}

/// "Recommended", in the brand colour.
struct RecommendedBadge: View {
    var body: some View {
        Text(L.t("Recommended", "推荐"))
            .font(.system(size: 10, weight: .semibold))
            .foregroundStyle(Theme.brand)
            .padding(.horizontal, 6)
            .frame(height: 18)
            .background(Capsule().fill(Theme.brand.opacity(0.14)))
    }
}

/// A Raycast-style dropdown: the chosen terminal's icon and name in a soft
/// filled capsule; a click opens our floating list (search on top, grey
/// highlight on the chosen one), not the system's blue menu.
struct TerminalPicker: View {
    let prefs: TerminalPrefs

    var body: some View {
        Dropdown(
            options: { () -> [DropdownOption<Terminal>] in
                prefs.rescan()
                return prefs.installed.map { t in
                    DropdownOption(id: t.terminal, title: t.terminal.displayName,
                                   image: TerminalPrefs.icon(t, size: 20),
                                   badge: t.terminal.isRecommended ? L.t("Recommended", "推荐") : nil)
                }
            },
            selection: prefs.current?.terminal,
            pick: { prefs.choose($0) }
        ) {
            if let current = prefs.current {
                Image(nsImage: TerminalPrefs.icon(current, size: 18))
                    .resizable().interpolation(.high).frame(width: 18, height: 18)
                Text(current.terminal.displayName)
            } else {
                Text(L.t("None found", "一个也没找到")).foregroundStyle(.secondary)
            }
        }
        .accessibilityLabel(L.t("Terminal: \(prefs.current?.terminal.displayName ?? "")", "终端：\(prefs.current?.terminal.displayName ?? "")"))
    }
}

/// The trigger of every settings dropdown: label and a chevron in a soft
/// capsule; opens a `DropdownWindow` right under itself.
struct Dropdown<ID: Hashable, Label: View>: View {
    let options: () -> [DropdownOption<ID>]
    let selection: ID?
    let pick: (ID) -> Void
    @ViewBuilder let label: () -> Label
    @State private var hovering = false
    @State private var open = false
    @State private var anchor = AnchorBox()

    var body: some View {
        Button { toggle() } label: {
            HStack(spacing: 8) {
                label().font(.system(size: 13, weight: .medium)).lineLimit(1).fixedSize()
                Image(systemName: open ? "chevron.up" : "chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 11)
            .frame(height: 30)
            .background(Capsule().fill(Color.primary.opacity(hovering || open ? 0.10 : 0.06)))
            .contentShape(Capsule())
            .background(AnchorView(box: anchor))
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }

    private func toggle() {
        if open { DropdownWindow.close(); return }
        guard let view = anchor.view, let window = view.window else { return }
        let rect = window.convertToScreen(view.convert(view.bounds, to: nil))
        open = true
        DropdownWindow.show(below: rect, content: DropdownList(options: options(), selection: selection) { id in
            pick(id)
            DropdownWindow.close()
        }) {
            open = false
        }
    }
}

/// Holds the trigger's NSView so the list can be placed under it.
final class AnchorBox {
    weak var view: NSView?
}

struct AnchorView: NSViewRepresentable {
    let box: AnchorBox
    func makeNSView(context: Context) -> NSView {
        let v = NSView()
        box.view = v
        return v
    }
    func updateNSView(_ nsView: NSView, context: Context) { box.view = nsView }
}

/// The floating list: a borderless glass panel with no arrow, under the
/// trigger, closing on a click elsewhere, Esc, or a choice.
@MainActor
enum DropdownWindow {
    private static var panel: NSPanel?
    private static var monitors: [Any] = []
    private static var onClose: (() -> Void)?
    /// The trigger, in screen points: a click there is the trigger's own
    /// toggle, not "elsewhere".
    private static var anchorRect: NSRect = .zero
    /// Room around the list for its shadow (radius 8, offset 3, ×2 + slack).
    private static let margin: CGFloat = 28

    static func show<V: View>(below anchor: NSRect, content: V, onClose: @escaping () -> Void) {
        close()
        anchorRect = anchor
        let host = NSHostingView(rootView: content
            .glassPanel(cornerRadius: 16)
            // A soft shadow that follows the rounded shape; the window's own
            // shadow would outline its rectangle.
            .shadow(color: .black.opacity(0.12), radius: 8, y: 3)
            // The shadow must fit inside the window: one cut off at the
            // window's edge shows up as a hard-edged rectangle.
            .padding(Self.margin))
        host.layoutSubtreeIfNeeded()
        let size = host.fittingSize
        let p = DropdownPanel(contentRect: NSRect(origin: .zero, size: size), styleMask: [.borderless, .nonactivatingPanel],
                         backing: .buffered, defer: false)
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = false
        p.level = .popUpMenu
        p.contentView = host
        // Right edges line up, just below the trigger, kept on screen.
        var origin = NSPoint(x: anchor.maxX - size.width + margin, y: anchor.minY - size.height + margin - 6)
        if let screen = NSScreen.screens.first(where: { $0.frame.contains(anchor.origin) }) ?? NSScreen.main {
            origin.x = min(max(origin.x, screen.visibleFrame.minX), screen.visibleFrame.maxX - size.width)
            origin.y = max(origin.y, screen.visibleFrame.minY)
        }
        p.setFrameOrigin(origin)
        p.makeKeyAndOrderFront(nil)
        panel = p
        self.onClose = onClose
        monitors.append(NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { e in
            if e.window !== panel, !anchorRect.contains(NSEvent.mouseLocation) { close() }
            return e
        } as Any)
        monitors.append(NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { _ in
            Task { @MainActor in close() }
        } as Any)
        monitors.append(NSEvent.addLocalMonitorForEvents(matching: .keyDown) { e in
            if e.keyCode == 53 { close(); return nil } // Esc
            return e
        } as Any)
    }

    static func close() {
        monitors.forEach { NSEvent.removeMonitor($0) }
        monitors.removeAll()
        panel?.orderOut(nil)
        panel = nil
        let done = onClose
        onClose = nil
        done?()
    }
}

/// One choice in a `DropdownList`.
struct DropdownOption<ID: Hashable>: Identifiable {
    let id: ID
    let title: String
    var image: NSImage?
    var symbol: String?
    var badge: String?
}

/// The list a dropdown opens: a search field, then roomy rows with icons;
/// the chosen one sits on a grey block (Raycast's look, no tick).
struct DropdownList<ID: Hashable>: View {
    let options: [DropdownOption<ID>]
    let selection: ID?
    let pick: (ID) -> Void
    @State private var hovered: ID?
    @State private var query = ""
    @FocusState private var searching: Bool

    private var shown: [DropdownOption<ID>] {
        let q = query.lowercased().trimmingCharacters(in: .whitespaces)
        return q.isEmpty ? options : options.filter { $0.title.lowercased().contains(q) }
    }

    var body: some View {
        VStack(spacing: 0) {
            TextField(L.t("Search…", "搜索…"), text: $query)
                .textFieldStyle(.plain)
                .font(.system(size: 14))
                .focused($searching)
                .onSubmit { if let first = shown.first { pick(first.id) } }
                .padding(.horizontal, 16)
                .frame(height: 44)
            Rectangle().fill(Color.primary.opacity(0.08)).frame(height: 1)
            VStack(spacing: 2) {
                ForEach(shown) { option in
                    Button { pick(option.id) } label: {
                        HStack(spacing: 12) {
                            if let image = option.image {
                                Image(nsImage: image).resizable().interpolation(.high).frame(width: 20, height: 20)
                            } else if let symbol = option.symbol {
                                Image(systemName: symbol).font(.system(size: 14)).frame(width: 20)
                            }
                            Text(option.title).font(.system(size: 14))
                            if let badge = option.badge {
                                Text(badge)
                                    .font(.system(size: 10, weight: .medium))
                                    .foregroundStyle(.secondary)
                                    .padding(.horizontal, 6)
                                    .frame(height: 17)
                                    .background(Capsule().fill(Color.primary.opacity(0.07)))
                            }
                            Spacer(minLength: 24)
                        }
                        .padding(.horizontal, 12)
                        .frame(height: 40)
                        .background(
                            RoundedRectangle(cornerRadius: 10)
                                .fill(Color.primary.opacity(option.id == selection ? 0.10 : (hovered == option.id ? 0.05 : 0)))
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(PressStyle())
                    .focusable(false)
                    .onHover { inside in hovered = inside ? option.id : (hovered == option.id ? nil : hovered) }
                }
                if shown.isEmpty {
                    Text(L.t("Nothing matches", "没有对得上的"))
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                        .frame(height: 40)
                }
            }
            .padding(8)
        }
        .frame(width: 260)
        .focusEffectDisabled()
        .onAppear { searching = true }
    }
}

/// A borderless panel that can take typing (for the dropdown's search field).
final class DropdownPanel: NSPanel {
    override var canBecomeKey: Bool { true }
}
