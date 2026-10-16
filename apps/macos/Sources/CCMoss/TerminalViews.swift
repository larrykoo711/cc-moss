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
                .help(L.t("Dismiss (Esc)", "关闭（Esc）"))
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

/// A Raycast-style dropdown: the chosen terminal's icon and name; a click
/// opens a menu of the installed ones, each with its own app icon.
struct TerminalPicker: View {
    let prefs: TerminalPrefs
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: popUp) {
            HStack(spacing: 7) {
                if let current = prefs.current {
                    Image(nsImage: TerminalPrefs.icon(current, size: 18))
                        .resizable().interpolation(.high).frame(width: 18, height: 18)
                    Text(current.terminal.displayName).font(.system(size: 12, weight: .medium)).lineLimit(1).fixedSize()
                    if current.terminal.isRecommended { RecommendedBadge() }
                } else {
                    Text(L.t("None found", "一个也没找到")).font(.system(size: 12)).foregroundStyle(.secondary)
                }
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.secondary)
            }
            .padding(.leading, 8)
            .padding(.trailing, 9)
            .frame(height: 28)
            .background(RoundedRectangle(cornerRadius: 7).fill(Theme.selection(scheme).opacity(hovering ? 1.6 : 1)))
            .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Theme.hairline(scheme)))
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
        .help(L.t("Choose the terminal sessions open in", "选择打开会话用的终端"))
        .accessibilityLabel(L.t("Terminal: \(prefs.current?.terminal.displayName ?? "")", "终端：\(prefs.current?.terminal.displayName ?? "")"))
    }

    private func popUp() {
        prefs.rescan()
        let menu = NSMenu()
        let target = MenuTarget { prefs.choose($0) }
        var selected: NSMenuItem?
        for t in prefs.installed {
            let suffix = t.terminal.isRecommended ? L.t("  ·  Recommended", "  ·  推荐") : ""
            let item = NSMenuItem(title: t.terminal.displayName + suffix, action: #selector(MenuTarget.pick(_:)), keyEquivalent: "")
            item.target = target
            item.representedObject = t.terminal.rawValue
            item.image = TerminalPrefs.icon(t, size: 16)
            if t.terminal == prefs.current?.terminal {
                item.state = .on
                selected = item
            }
            menu.addItem(item)
        }
        // Modal until the menu closes, so `target` lives long enough.
        menu.popUp(positioning: selected, at: NSEvent.mouseLocation, in: nil)
        withExtendedLifetime(target) {}
    }
}

private final class MenuTarget: NSObject {
    let onPick: (Terminal) -> Void
    init(_ onPick: @escaping (Terminal) -> Void) { self.onPick = onPick }

    @objc func pick(_ sender: NSMenuItem) {
        if let raw = sender.representedObject as? String, let t = Terminal(rawValue: raw) { onPick(t) }
    }
}
