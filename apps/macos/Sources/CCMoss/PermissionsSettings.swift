import AppKit
import CoreServices
import MossKit
import SwiftUI

/// One place that knows where every permission stands: Accessibility (from
/// the shortcut center), notifications (from NotificationStatus) and
/// Automation for the terminal sessions open in.
@MainActor @Observable
final class PermissionCenter {
    static let shared = PermissionCenter()

    private(set) var automation: [AutomationTarget: Grant] = [:]
    /// Allowed before, gone now: an update took it away.
    private(set) var accessLost = false
    /// Screenshots pin what they show instead of asking macOS.
    @ObservationIgnored var pinned = false
    @ObservationIgnored private let memory = AccessibilityMemory()

    var accessibility: Grant {
        get { accessOverride ?? (HotKeyCenter.shared.trust.allowed ? .allowed : .denied) }
        set { accessOverride = newValue }
    }

    private var accessOverride: Grant?

    func accessibilityNeeded(_ terminals: TerminalPrefs) -> Bool {
        PermissionNeeds.accessibility(shortcut: HotKeyCenter.shared.shortcut,
                                      terminal: terminals.current?.terminal, style: terminals.openStyle)
    }

    func targets(_ terminals: TerminalPrefs) -> [AutomationTarget] {
        terminals.current.map { PermissionNeeds.automation(for: $0, style: terminals.openStyle) } ?? []
    }

    /// At launch, before anything records the new state.
    func checkAtLaunch(terminals: TerminalPrefs) {
        let trusted = AXIsProcessTrusted()
        accessLost = memory.lost(trusted: trusted, needed: accessibilityNeeded(terminals))
        memory.record(trusted: trusted)
    }

    /// Look again (the settings page does this every couple of seconds).
    func refresh(terminals: TerminalPrefs) {
        guard !pinned else { return }
        HotKeyCenter.shared.recheckTrust()
        let trusted = accessibility == .allowed
        memory.record(trusted: trusted)
        if trusted { accessLost = false }
        var found: [AutomationTarget: Grant] = [:]
        for target in targets(terminals) { found[target] = Self.check(target) }
        if found != automation { automation = found }
    }

    func pin(accessibility: Grant, automation: [AutomationTarget: Grant], lost: Bool = false) {
        pinned = true
        self.accessibility = accessibility
        self.automation = automation
        accessLost = lost
    }

    /// How many need you; the sidebar shows a dot when it's more than none.
    func attention(terminals: TerminalPrefs) -> Int {
        PermissionSummary.attention(accessibility: accessibility,
                                    accessibilityNeeded: accessibilityNeeded(terminals),
                                    notifications: NotificationStatus.shared.auth,
                                    automation: targets(terminals).map { automation[$0] ?? .unknown })
    }

    /// The banner's "Later": no more reminders until it's allowed again.
    func dismissLost() {
        memory.dismiss()
        accessLost = false
    }

    static func allowAccessibility() {
        HotKeyCenter.shared.askForAccessibility()
        HotKeyCenter.openAccessibilitySettings()
    }

    /// Asks macOS without prompting. It can only tell while the other app is
    /// running; otherwise the answer is "unknown" and macOS asks on first use.
    static func check(_ target: AutomationTarget) -> Grant {
        var address = AEAddressDesc()
        let id = Array(target.bundleID.utf8)
        let made = id.withUnsafeBytes { AECreateDesc(typeApplicationBundleID, $0.baseAddress, $0.count, &address) }
        guard made == noErr else { return .unknown }
        defer { AEDisposeDesc(&address) }
        return Grant(automationStatus: AEDeterminePermissionToAutomateTarget(&address, typeWildCard, typeWildCard, false))
    }
}

/// Settings › Permissions: everything cc-moss asks macOS for, where it
/// stands, and the one thing to do about it.
struct PermissionsSettings: View {
    let model: AppModel
    var center: PermissionCenter = .shared

    var body: some View {
        SettingsCard(title: L.t("System", "系统")) {
            PermissionRow(kind: .accessibility, needed: center.accessibilityNeeded(model.terminals))
            PermissionRow(kind: .notifications, divider: false)
        }
        SettingsCard(title: L.t("Automation", "自动化")) {
            let targets = center.targets(model.terminals)
            if targets.isEmpty {
                let name = model.terminals.current?.terminal.displayName ?? L.t("Your terminal", "你的终端")
                CardRow(title: L.t("\(name) needs none", "\(zhPad(name, leading: false))不需要这一项"),
                        detail: L.t("cc-moss starts it from the command line.", "cc-moss 从命令行启动它，不用自动化权限。"),
                        divider: false) { EmptyView() }
            } else {
                ForEach(Array(targets.enumerated()), id: \.element) { i, target in
                    PermissionRow(kind: .automation(target), divider: i < targets.count - 1)
                }
            }
        }
        Text(L.t("After an update, Accessibility can look ticked yet do nothing: cc-moss isn't signed with an Apple developer ID yet, so macOS sees each version as a new app. Click Allow and tick it once more; cc-moss clears the old entry first.",
                 "升级后，辅助功能可能看着是勾上的，其实不起作用：cc-moss 还没有 Apple 开发者签名，macOS 把每个新版本都当成新 App。点「去允许」再勾一次就好，旧的那条 cc-moss 会先清掉。"))
            .font(.system(size: 11))
            .foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 12)
            .task {
                while !Task.isCancelled {
                    center.refresh(terminals: model.terminals)
                    if !center.pinned { await NotificationStatus.shared.refresh() }
                    try? await Task.sleep(for: .seconds(2))
                }
            }
    }
}

/// A permission cc-moss uses.
enum PermissionKind: Equatable {
    case accessibility
    case notifications
    case automation(AutomationTarget)
}

/// One permission: what it's for, where it stands, and a button when there's
/// something to do. The Permissions page lists them all; other pages show the
/// same row, in their own words, where the missing one is felt.
struct PermissionRow: View {
    let kind: PermissionKind
    /// Something uses it right now; when not, a missing one is no warning.
    var needed = true
    var title: String?
    var detail: String?
    var divider = true
    var center: PermissionCenter = .shared
    var notifications: NotificationStatus = .shared

    var body: some View {
        CardRow(title: title ?? defaultTitle, detail: detail ?? defaultDetail, divider: divider) {
            HStack(spacing: 10) {
                StatusLabel(text: statusText, symbol: symbol, tint: tint)
                button
            }
        }
    }

    // MARK: - State

    private var grant: Grant {
        switch kind {
        case .accessibility: center.accessibility
        case .notifications:
            switch notifications.auth {
            case .allowed: .allowed
            case .denied, .unavailable: .denied
            case .notDetermined, .unknown: .notAsked
            }
        case let .automation(target): center.automation[target] ?? .unknown
        }
    }

    private var unavailable: Bool { kind == .notifications && notifications.auth == .unavailable }

    private var statusText: String {
        if unavailable { return L.t("Unavailable", "用不了") }
        if kind == .accessibility, center.accessLost, grant != .allowed { return L.t("Lost in the update", "升级后失效") }
        return switch grant {
        case .allowed: L.t("Allowed", "已允许")
        case .denied: L.t("Not allowed", "未允许")
        case .notAsked: L.t("Not asked yet", "还没问过")
        case .unknown: L.t("Asks on first use", "用到时再问")
        }
    }

    private var symbol: String {
        switch grant {
        case .allowed: "checkmark.circle.fill"
        case .denied: needed ? "exclamationmark.triangle.fill" : "minus.circle"
        case .notAsked, .unknown: "circle.dashed"
        }
    }

    private var tint: Color {
        switch grant {
        case .allowed: .green
        case .denied: needed ? Theme.amber : .secondary
        case .notAsked, .unknown: .secondary
        }
    }

    // MARK: - Words

    private var defaultTitle: String {
        switch kind {
        case .accessibility: L.t("Accessibility", "辅助功能")
        case .notifications: L.t("Notifications", "通知")
        case .automation(.systemEvents): L.t("Control System Events", "控制 System Events")
        case let .automation(.terminal(t)): L.t("Control \(t.displayName)", "控制「\(t.displayName)」")
        }
    }

    private var defaultDetail: String {
        switch kind {
        case .accessibility:
            let k = HotKeyCenter.shared.shortcut.tapKey?.glyph ?? "⌃"
            return L.t("Lets a double \(k) work in any app, and opens sessions as tabs in Terminal.",
                       "在任何 App 里连按 \(k) 都能唤出面板；在「终端」里用标签页打开会话。")
                + (needed ? "" : L.t(" Your settings don't use it now.", "现在的设置用不到它。"))
        case .notifications:
            if unavailable {
                return L.t("Move cc-moss into Applications to use them.", "把 cc-moss 放进「应用程序」文件夹才能用。")
            }
            return L.t("Banners when a session needs you or stops.", "会话等你或断了时，用系统横幅告诉你。")
        case .automation(.systemEvents):
            return L.t("Presses ⌘T in Terminal for a new tab.", "在「终端」里按 ⌘T 开新标签页。")
        case .automation(.terminal):
            return L.t("Opens your sessions in it.", "在它里面打开会话。")
        }
    }

    // MARK: - The one thing to do

    @ViewBuilder private var button: some View {
        switch (kind, grant) {
        case (.accessibility, .denied):
            SoftButton(title: L.t("Allow", "去允许"), icon: "hand.raised", prominent: needed) {
                PermissionCenter.allowAccessibility()
            }
        case (.notifications, .notAsked):
            SoftButton(title: L.t("Allow", "去允许"), icon: "bell.badge", prominent: true) {
                Task { await notifications.request() }
            }
            .tip(L.t("macOS asks you once", "macOS 会问你一次"))
        case (.notifications, .denied) where !unavailable:
            SoftButton(title: L.t("Open Settings", "去设置"), icon: Icons.settings) {
                NotificationStatus.openSystemSettings()
            }
            .tip(L.t("System Settings › Notifications", "系统设置 › 通知"))
        case (.automation, .denied):
            SoftButton(title: L.t("Open Settings", "去设置"), icon: Icons.settings) {
                NSWorkspace.shared.open(Opener.automationSettings)
            }
            .tip(L.t("Privacy & Security › Automation", "隐私与安全性 › 自动化"))
        default:
            EmptyView()
        }
    }
}

/// A small coloured symbol and a word: where a permission stands.
private struct StatusLabel: View {
    let text: String
    let symbol: String
    let tint: Color

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: symbol).font(.system(size: 12, weight: .semibold)).foregroundStyle(tint)
            Text(text).font(.system(size: 12)).foregroundStyle(.secondary)
        }
        .lineLimit(1)
        .fixedSize()
        .accessibilityElement(children: .combine)
    }
}

/// At the top of the panel when an update took Accessibility away: the
/// shortcut went quiet, and this says why before you go looking.
struct AccessLostBanner: View {
    var center: PermissionCenter = .shared
    @Environment(\.colorScheme) private var scheme

    @MainActor static func shows() -> Bool {
        let center = PermissionCenter.shared
        return center.accessLost && center.accessibility != .allowed
    }

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: 6).fill(Theme.amber.opacity(scheme == .dark ? 0.22 : 0.14))
                Image(systemName: "hand.raised").font(.system(size: 11, weight: .semibold)).foregroundStyle(Theme.amber)
            }
            .frame(width: 24, height: 24)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(L.t("Allow Accessibility again", "需要重新允许辅助功能"))
                    .font(.system(size: 13, weight: .medium))
                Text(L.t("The update turned it off.", "升级后失效了，快捷键暂时不响应。"))
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }
            .lineLimit(1)
            Spacer(minLength: 8)
            SoftButton(title: L.t("Allow", "去允许"), icon: "hand.raised", prominent: true) {
                PermissionCenter.allowAccessibility()
            }
            Button { center.dismissLost() } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .frame(width: 20, height: 20)
                    .contentShape(Rectangle())
            }
            .buttonStyle(PressStyle())
            .tip(L.t("Not now; Settings › Permissions has it", "先不管，设置 › 权限里随时能开"))
            .accessibilityLabel(L.t("Not now", "先不管"))
        }
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .frame(height: 44)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Theme.amber.opacity(scheme == .dark ? 0.14 : 0.08)))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.amber.opacity(0.18)))
    }
}
