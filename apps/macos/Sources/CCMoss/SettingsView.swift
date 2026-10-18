import AppKit
import MossKit
import ServiceManagement
import SwiftUI

/// The settings window, after Raycast's (measured from its settings
/// stylesheet): a floating glass sidebar under the window buttons with a
/// large search field and roomy section rows (no account row: cc-moss has
/// no accounts; who it is lives on About); on the right a
/// back/forward capsule and grouped rows (2.5% tint, 46 pt rows, 12 pt
/// insets, 5% hairlines, 32 pt group titles). Every change applies at once.
/// Sections arrive with the features they configure; none is a placeholder.
struct SettingsView: View {
    @Bindable var model: AppModel
    @State var tab: Tab = .general
    /// Canned `alias ls --json` for screenshots.
    var aliasDemo: AliasList?
    /// A canned daemon check for screenshots.
    var daemonDemo: DaemonCheck?
    /// A canned Moss check for screenshots.
    var mossDemo: MossCheck?
    @State private var search = ""
    @FocusState private var searchFocused: Bool
    @State private var back: [Tab] = []
    @State private var forward: [Tab] = []
    @Environment(\.colorScheme) private var scheme

    static let size = CGSize(width: 820, height: 560)

    /// The sections, in the sidebar's order. Everyday things first, one
    /// subject per page; About stands apart at the bottom of the list.
    enum Tab: String, CaseIterable, Hashable {
        case general, shortcuts, menuBar, mo, notifications, permissions, moss, about

        var title: String {
            switch self {
            case .general: L.t("General", "通用")
            case .menuBar: L.t("Menu Bar", "菜单栏")
            case .mo: L.t("Mo", "小莫")
            case .notifications: L.t("Notifications", "通知")
            case .shortcuts: L.t("Shortcuts", "快捷键")
            case .permissions: L.t("Permissions", "权限")
            case .moss: "Moss"
            case .about: L.t("About", "关于")
            }
        }

        var icon: String {
            switch self {
            case .general: Icons.general
            case .menuBar: "menubar.rectangle"
            case .mo: "face.smiling"
            case .notifications: Icons.notifications
            case .shortcuts: Icons.shortcuts
            case .permissions: Icons.permissions
            case .moss: "server.rack"
            case .about: "infinity"
            }
        }

        /// A little room above this one in the sidebar: it starts a group.
        var startsGroup: Bool { self == .about }

        /// Words that find this section from the search field.
        var keywords: [String] {
            switch self {
            case .general: [title, L.t("login startup language english chinese new session bypass permission workspace folder directory terminal ghostty iterm wezterm kitty alacritty",
                                       "开机 登录 启动 语言 中文 英文 english 新会话 bypass 权限 放手 工作区 目录 文件夹 终端 ghostty iterm wezterm kitty alacritty")]
            case .menuBar: [title, L.t("icon number count motion animation cpu", "图标 数字 数量 动态 动画 cpu 状态栏")]
            case .mo: [title, "Mo", L.t("pet desktop companion size interaction quiet everyday active voice speech speak sound chime read aloud",
                                       "宠物 桌面 小伙伴 大小 互动 安静 日常 积极 声音 语音 说话 朗读 播报 提示音 voice")]
            case .notifications: [title, L.t("alert banner notification center needs you permission question stopped interrupted finished turn bring back restore mo pet do not disturb",
                                             "提醒 通知中心 横幅 需要你确认 权限 提问 断了 中断 做完 这一轮 找回 小莫 宠物 勿扰 notification")]
            case .shortcuts: [title, "alias", L.t("shortcut hotkey keyboard double tap control panel command terminal short name type",
                                                  "快捷键 热键 键盘 连按 唤出 面板 短命令 终端 别名 输入 alias")]
            case .permissions: [title, L.t("privacy security accessibility automation notifications allow system events terminal tab double tap",
                                           "隐私 安全 辅助功能 自动化 通知 允许 授权 system events 终端 标签页 连按")]
            case .moss: [title, L.t("moss resident session always on off switch restart daemon background launchd login running start keep alive",
                                     "Moss 常驻 会话 一直开着 开关 关闭 重启 守护进程 后台 登录 运行 启动 launchd")]
            case .about: [title, "cc-moss", "GitHub", L.t("version update license command line program path", "版本 更新 许可 命令行 程序 位置 路径")]
            }
        }
    }

    /// Open on a given section (Mo's menu opens on Mo): taken when the
    /// window appears, or at once if it is already open.
    @MainActor static func show(_ tab: Tab) {
        pendingTab = tab
        NotificationCenter.default.post(name: showTab, object: tab.rawValue)
    }

    @MainActor private static var pendingTab: Tab?
    static let showTab = Notification.Name("cc-moss.settings.showTab")

    private var tabs: [Tab] {
        let q = search.lowercased().trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return Tab.allCases }
        return Tab.allCases.filter { $0.keywords.joined(separator: " ").lowercased().contains(q) }
    }

    private func go(_ t: Tab) {
        guard t != tab else { return }
        back.append(tab)
        forward.removeAll()
        tab = t
    }

    var body: some View {
        HStack(spacing: 0) {
            sidebar
                .frame(width: 252)
                .padding(8)
            VStack(alignment: .leading, spacing: 0) {
                // The page's name beside back and forward, as in System Settings.
                HStack(spacing: 12) {
                    NavCapsule(
                        canGoBack: !back.isEmpty, canGoForward: !forward.isEmpty,
                        goBack: { if let t = back.popLast() { forward.append(tab); tab = t } },
                        goForward: { if let t = forward.popLast() { back.append(tab); tab = t } }
                    )
                    Text(tab.title)
                        .font(.system(size: 15, weight: .semibold))
                        .contentTransition(.opacity)
                        .animation(.easeOut(duration: 0.15), value: tab)
                }
                .padding(.top, 10)
                .padding(.bottom, 14)
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        switch tab {
                        case .general: GeneralSettings(model: model)
                        case .menuBar: MenuBarSettings(model: model)
                        case .mo: PetSettings()
                        case .notifications: NotificationSettings(model: model)
                        case .shortcuts: ShortcutsSettings(model: model, demo: aliasDemo)
                        case .permissions: PermissionsSettings(model: model)
                        case .moss: MossSettings(model: model, demo: daemonDemo, mossDemo: mossDemo)
                        case .about: AboutSettings(model: model)
                        }
                    }
                    .padding(.bottom, 20)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .scrollIndicators(.never)
            }
            .padding(.leading, 6)
            .padding(.trailing, 20)
        }
        // Fill whatever height the window has: the hidden title bar still
        // counts 28 pt, and a fixed height left a strip at the bottom.
        .frame(width: Self.size.width)
        .frame(minHeight: Self.size.height, maxHeight: .infinity)
        .background(Theme.surface(scheme))
        .background(.regularMaterial)
        .ignoresSafeArea()
        .focusEffectDisabled()
        // Every layout pass puts the window buttons back inside the sidebar
        // and keeps the window exactly our size; doing it once on open was
        // undone by the next layout.
        .background(WindowLayoutHook())
        .tooltipLayer()
        // No raising here: whoever opens Settings raises it. A language switch
        // rebuilds this view, and raising on appear popped a closed Settings up.
        .onAppear {
            if let t = Self.pendingTab {
                Self.pendingTab = nil
                tab = t
            }
        }
        .onChange(of: search) { _, _ in
            if let first = tabs.first, !tabs.contains(tab) { go(first) }
        }
        .onReceive(NotificationCenter.default.publisher(for: Self.showTab)) { note in
            Self.pendingTab = nil
            if let raw = note.object as? String, let t = Tab(rawValue: raw) { go(t) }
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 2) {
            Spacer().frame(height: 40) // the window buttons sit here
            // Raycast's large rounded search field.
            // Focused, it deepens like Raycast's: a darker fill and a
            // darker magnifier, eased in.
            HStack(spacing: 8) {
                Image(systemName: Icons.search)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(searchFocused ? .primary : .secondary)
                TextField(L.t("Search settings…", "搜索设置…"), text: $search)
                    .textFieldStyle(.plain)
                    .font(.system(size: 14))
                    .focused($searchFocused)
                if !search.isEmpty {
                    Button { search = "" } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 13))
                            .foregroundStyle(.tertiary)
                    }
                    .buttonStyle(PressStyle())
                    .focusable(false)
                    .tip(L.t("Clear", "清空"))
                }
            }
            .padding(.horizontal, 12)
            .frame(height: 36)
            .background(
                Capsule().fill(Color.primary.opacity(searchFocused
                    ? (scheme == .dark ? 0.15 : 0.11)
                    : (scheme == .dark ? 0.08 : 0.06)))
            )
            .contentShape(Capsule())
            .onTapGesture { searchFocused = true }
            // The whole capsule is the field: the text cursor all over it.
            .cursor(.iBeam)
            .animation(.easeOut(duration: 0.15), value: searchFocused)
            .padding(.bottom, 10)
            ForEach(tabs, id: \.self) { t in
                if t.startsGroup, search.isEmpty {
                    Rectangle().fill(Color.primary.opacity(0.07)).frame(height: 1)
                        .padding(.horizontal, 8).padding(.vertical, 6)
                }
                SidebarItem(tab: t, selected: t == tab,
                            badge: t == .permissions && PermissionCenter.shared.attention(terminals: model.terminals) > 0) { go(t) }
            }
            Spacer()
        }
        .padding(.horizontal, 10)
        .padding(.bottom, 10)
        .frame(maxHeight: .infinity)
        .glassPanel(cornerRadius: 18)
    }
}

/// A sidebar entry: a plain symbol and the section's name, the way Finder's sidebar does it.
struct SidebarItem: View {
    let tab: SettingsView.Tab
    let selected: Bool
    /// Something here needs you (a permission switched off).
    var badge = false
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme
    /// Spelled out per scheme: on the glass sidebar the semantic colours
    /// went through vibrancy and came out near-invisible.
    private var ink: Color { scheme == .dark ? .white : .black }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: tab.icon)
                    .font(.system(size: 14, weight: .regular))
                    .foregroundStyle(ink.opacity(selected ? 0.9 : 0.55))
                    .frame(width: 22, height: 22)
                Text(tab.title).font(.system(size: 14)).foregroundStyle(ink.opacity(selected ? 1 : 0.85))
                Spacer(minLength: 0)
                if badge {
                    Circle().fill(Theme.amber).frame(width: 7, height: 7)
                        .padding(.trailing, 4)
                        .accessibilityLabel(L.t("Needs you", "需要处理"))
                }
            }
            .padding(.horizontal, 6)
            .frame(height: 36)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(Color.primary.opacity(selected ? 0.08 : (hovering ? 0.04 : 0)))
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }
}

/// Back and forward through the sections you visited, in one glass capsule.
struct NavCapsule: View {
    let canGoBack: Bool
    let canGoForward: Bool
    let goBack: () -> Void
    let goForward: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            arrow("chevron.left", enabled: canGoBack, action: goBack)
                .tip(L.t("Back", "后退"))
            Rectangle().fill(Color.primary.opacity(0.12)).frame(width: 1, height: 16)
            arrow("chevron.right", enabled: canGoForward, action: goForward)
                .tip(L.t("Forward", "前进"))
        }
        .frame(height: 32)
        .glassPanel(cornerRadius: 16)
    }

    private func arrow(_ symbol: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(enabled ? .primary : .tertiary)
                .frame(width: 34, height: 32)
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .disabled(!enabled)
    }
}

extension View {
    /// Liquid Glass on macOS 26 and later (what Raycast's sidebar uses);
    /// frosted material before that.
    @ViewBuilder func glassPanel(cornerRadius: CGFloat) -> some View {
        if #available(macOS 26.0, *) {
            self.glassEffect(.regular, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        } else {
            self.background(.thinMaterial, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        }
    }
}

/// A coloured rounded square with a white symbol, like System Settings.
struct IconTile: View {
    let symbol: String
    let colors: [Color]
    var size: CGFloat = 20

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.27, style: .continuous)
                .fill(LinearGradient(colors: colors, startPoint: .top, endPoint: .bottom))
            Image(systemName: symbol)
                .font(.system(size: size * 0.52, weight: .semibold))
                .foregroundStyle(.white)
        }
        .frame(width: size, height: size)
    }
}

/// A group of rows: Raycast's 2.5% tint, no border, 12 pt corners, with an
/// optional title above (32 pt tall, inset 12 pt).
struct SettingsCard<Content: View>: View {
    var title: String?
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let title {
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .frame(minHeight: 30, alignment: .center)
                    .padding(.leading, 12)
            }
            VStack(spacing: 0) {
                content()
            }
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.primary.opacity(0.025)))
        }
    }
}

/// One row: title and description on the left, control on the right; at
/// least 46 pt tall with 12 pt padding, a 5% hairline inset 12 pt below.
struct CardRow<Control: View>: View {
    let title: String
    var detail: String?
    var divider = true
    @ViewBuilder let control: () -> Control

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(title).font(.system(size: 14))
                    if let detail {
                        Text(detail)
                            .font(.system(size: 12))
                            .foregroundStyle(Theme.caption)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 12)
                control()
            }
            .padding(12)
            .frame(minHeight: 46)
            if divider {
                Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
            }
        }
    }
}

/// A filled text field, Raycast's style rather than the bezelled default.
struct FilledField: View {
    let placeholder: String
    @Binding var text: String
    var monospaced = false
    /// Take the keyboard as soon as it appears (the panel's forms).
    var autofocus = false
    /// The form's one field: taller, larger type, text selected on arrival
    /// so typing replaces it.
    var large = false
    /// Outline colour for a value that needs a second look (a taken name).
    var tint: Color?
    var onSubmit: () -> Void = {}
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        TextField(placeholder, text: $text)
            .textFieldStyle(.plain)
            .font(.system(size: large ? 14 : 12, design: monospaced ? .monospaced : .default))
            .focused($focused)
            .onSubmit(onSubmit)
            .padding(.horizontal, large ? 12 : 10)
            .frame(height: large ? 36 : 28)
            .background(RoundedRectangle(cornerRadius: 7).fill(Theme.selection(scheme).opacity(0.8)))
            .overlay(
                RoundedRectangle(cornerRadius: 7)
                    .strokeBorder(tint?.opacity(0.8) ?? (focused ? Theme.brand.opacity(0.7) : Theme.hairline(scheme)),
                                  lineWidth: focused || tint != nil ? 1.5 : 1)
            )
            .animation(.easeOut(duration: 0.12), value: focused)
            .onAppear {
                guard autofocus else { return }
                // A moment later: on appearing the field isn't in the window
                // yet (the form is still fading in), so focus set now is lost.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { focused = true }
                if large {
                    DispatchQueue.main.async { NSApp.sendAction(#selector(NSResponder.selectAll(_:)), to: nil, from: nil) }
                }
            }
            // The panel getting the keyboard back (after a dropdown or the
            // folder picker) hands it to the form's field again.
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { note in
                if autofocus, note.object is KeyPanel { focused = true }
            }
    }
}

/// A small rounded button with hover and press feedback.
struct SoftButton: View {
    let title: String
    let icon: String
    var prominent = false
    /// Its key, shown as a small keycap after the title (Raycast's action bar).
    var shortcut: String?
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: icon).font(.system(size: 11, weight: .medium))
                Text(title).font(.system(size: 12, weight: .medium)).lineLimit(1).fixedSize()
                if let shortcut {
                    Text(shortcut)
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(prominent ? Color.white.opacity(0.85) : Color.secondary)
                        .lineLimit(1)
                        .fixedSize()
                        .padding(.horizontal, 4)
                        .frame(minWidth: 18, minHeight: 16)
                        .background(RoundedRectangle(cornerRadius: 4, style: .continuous)
                            .fill(prominent ? Color.white.opacity(0.2) : Color.primary.opacity(0.07)))
                        .padding(.leading, 1)
                }
            }
            .foregroundStyle(prominent ? Color.white : Color.primary)
            .padding(.horizontal, 11)
            .frame(height: 28)
            .background(
                RoundedRectangle(cornerRadius: 7)
                    .fill(prominent
                          ? Theme.brand.opacity(hovering ? 0.9 : 1)
                          : Theme.selection(scheme).opacity(hovering ? 1.6 : 1))
            )
            .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(prominent ? .clear : Theme.hairline(scheme)))
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }
}

/// General: starting up, the language, how new sessions start, and where they open.
struct GeneralSettings: View {
    @Bindable var model: AppModel

    @State private var openAtLogin = LoginItem.isEnabled
    @State private var loginError: String?
    @State private var language = LanguageState.shared.choice.rawValue
    @State private var languageError: String?
    /// `claude.permission_mode` as last read or saved (bypass until read).
    @State private var permissionMode = PermissionMode.bypass
    @State private var bypass = true
    @State private var bypassError: String?

    var body: some View {
        SettingsCard(title: L.t("Startup", "启动")) {
            CardRow(
                title: L.t("Open at login", "开机自动打开"),
                detail: loginError ?? L.t("Starts with your Mac, so you don't have to remember.",
                                         "开机后自己出来，不用你记着。"),
                divider: false
            ) {
                Toggle("", isOn: $openAtLogin)
                    .toggleStyle(.switch)
                    .cursor(.pointingHand)
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: openAtLogin) { _, on in setOpenAtLogin(on) }
            }
        }
        SettingsCard(title: L.t("Language", "语言")) {
            CardRow(
                title: L.t("Language", "语言"),
                detail: languageError ?? L.t("Follow your Mac, or pick one. The terminal follows too.",
                                             "跟随系统，也可以固定成中文或英文。"),
                divider: false
            ) {
                SlidingPicker(selection: $language, options: [
                    (LangChoice.auto.rawValue, L.t("System", "跟随系统")),
                    (LangChoice.zh.rawValue, "中文"),
                    (LangChoice.en.rawValue, "English"),
                ])
                .onChange(of: language) { _, new in setLanguage(new) }
            }
        }
        SettingsCard(title: L.t("New sessions", "新会话")) {
            CardRow(
                title: L.t("Let it work on its own", "直接放手干"),
                detail: bypassError ?? L.t("New sessions don't stop to ask for permission (bypass).",
                                           "新会话不再逐条问你权限（bypass）。"),
                divider: false
            ) {
                Toggle("", isOn: $bypass)
                    .toggleStyle(.switch)
                    .cursor(.pointingHand)
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: bypass) { _, on in setBypass(on) }
            }
        }
        .task { await loadPermissionMode() }
        SettingsCard(title: L.t("Workspace", "工作区")) {
            WorkspaceRow()
        }
        TerminalSettings(prefs: model.terminals)
    }

    private func loadPermissionMode() async {
        guard let mode = try? await model.client?.permissionMode() else { return }
        permissionMode = mode
        bypass = mode.isBypass
    }

    private func setBypass(_ on: Bool) {
        let mode = PermissionMode(bypass: on)
        guard mode != permissionMode else { return }
        Task {
            do {
                guard let client = model.client else { throw MossClient.Failure.command(L.t("cc-moss not found", "没找到 cc-moss")) }
                try await client.setPermissionMode(mode)
                permissionMode = mode
                bypassError = nil
            } catch {
                bypass = permissionMode.isBypass
                bypassError = L.t("Couldn't save it: \(error)", "没存上：\(error)")
            }
        }
    }

    private func setLanguage(_ raw: String) {
        let choice = LangChoice(config: raw)
        guard choice != LanguageState.shared.choice else { return }
        Task {
            do {
                try await LanguageState.shared.choose(choice, client: model.client)
                languageError = nil
            } catch {
                language = LanguageState.shared.choice.rawValue
                languageError = L.t("Couldn't save it: \(error)", "没存上：\(error)")
            }
        }
    }

    private func setOpenAtLogin(_ on: Bool) {
        do {
            try LoginItem.set(on)
            loginError = nil
        } catch {
            loginError = L.t("Couldn't change it: \(error.localizedDescription)", "没改成：\(error.localizedDescription)")
        }
        openAtLogin = LoginItem.isEnabled
    }
}

/// Where the cc-moss program is: found in the usual places, or chosen.
struct ProgramRow: View {
    @Bindable var model: AppModel

    var body: some View {
        CardRow(
            title: L.t("cc-moss in Terminal", "终端里的 cc-moss"),
            detail: model.cliPath == nil
                ? L.t("Not found yet. Use Choose… to point to it.", "还没找到，点「选择…」指一下位置。")
                : L.t("Type cc-moss in any terminal to use it there.", "在终端里输入 cc-moss 也能用。"),
            divider: false
        ) {
            HStack(spacing: 8) {
                if UserDefaults.standard.string(forKey: "cliPath") != nil {
                    SoftButton(title: L.t("Automatic", "自动"), icon: "arrow.uturn.backward") { set(nil) }
                        .tip(L.t("Find it in the usual places again", "重新在常见位置自动找"))
                }
                SoftButton(title: L.t("Choose…", "选择…"), icon: Icons.choose) { choose() }
            }
        }
        .tip(model.cliPath.map { Grouping.tilde($0, home: NSHomeDirectory()) } ?? L.t("Not found", "没找到"))
    }

    private func set(_ path: String?) {
        if let path {
            UserDefaults.standard.set(path, forKey: "cliPath")
        } else {
            UserDefaults.standard.removeObject(forKey: "cliPath")
        }
        model.relocate()
    }

    private func choose() {
        FilePicker.choose(.program, from: NSApp.keyWindow, startingAt: model.cliPath.map { ($0 as NSString).deletingLastPathComponent },
                          message: L.t("Choose the cc-moss program", "选择 cc-moss 程序"),
                          prompt: L.t("Choose", "选择")) { set($0) }
    }
}

/// The app icon, drawn: the Continuum mark on the dark tile of docs/assets/icon.svg.
struct AppIconTile: View {
    var size: CGFloat = 96
    @State private var start = Date()

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.225, style: .continuous)
                .fill(LinearGradient(colors: [Color(red: 0.15, green: 0.16, blue: 0.18),
                                              Color(red: 0.07, green: 0.07, blue: 0.09)],
                                     startPoint: .top, endPoint: .bottom))
                .overlay(
                    RoundedRectangle(cornerRadius: size * 0.225, style: .continuous)
                        .strokeBorder(Color.white.opacity(0.08))
                )
                .shadow(color: .black.opacity(0.25), radius: 10, y: 4)
            TimelineView(.animation) { context in
                Continuum(phase: (context.date.timeIntervalSince(start) / 3).truncatingRemainder(dividingBy: 1))
                    .stroke(style: StrokeStyle(lineWidth: size * 0.07, lineCap: .round, lineJoin: .round))
                    .foregroundStyle(Theme.brand)
                    .frame(width: size * 0.75, height: size * 0.75)
            }
        }
        .frame(width: size, height: size)
    }
}

/// About: who cc-moss is, whether it is current, and where to learn more.
struct AboutSettings: View {
    @Bindable var model: AppModel
    @State private var cliVersion: String?
    @State private var update: UpdateState = .idle
    @State private var problem: String?
    @State private var aligning = false

    enum UpdateState: Equatable {
        case idle, checking
        case current(String)
        case available(UpdateInfo)
        case failed(String)
    }

    var body: some View {
        VStack(spacing: 22) {
            hero
            SettingsCard(title: L.t("Software Update", "软件更新")) {
                CardRow(title: updateTitle, detail: updateDetail) {
                    updateControl
                }
                ProgramRow(model: model)
            }
            SettingsCard(title: L.t("Learn More", "了解更多")) {
                LinkRow(icon: "sparkles", title: L.t("Show the Welcome Again", "再看一遍欢迎引导")) {
                    WelcomeWindow.show(model: model)
                }
                LinkRow(icon: "book", title: L.t("User Guide", "使用手册"),
                        url: L.t("https://github.com/larrykoo711/cc-moss/blob/main/docs/guide.en.md",
                                 "https://github.com/larrykoo711/cc-moss/blob/main/docs/guide.md"))
                LinkRow(icon: "doc.text", title: L.t("Release Notes", "更新记录"),
                        url: "https://github.com/larrykoo711/cc-moss/blob/main/CHANGELOG.md")
                LinkRow(icon: "exclamationmark.bubble", title: L.t("Report a Problem", "报告问题"),
                        url: "https://github.com/larrykoo711/cc-moss/issues/new/choose")
                LinkRow(icon: Icons.github, title: L.t("Source Code", "源代码"),
                        url: "https://github.com/larrykoo711/cc-moss", divider: false)
            }
            Text(L.t("Open source under MIT. Not affiliated with Anthropic.",
                     "MIT 开源。cc-moss 是个人独立项目，与 Anthropic 没有关联。"))
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
                .frame(maxWidth: .infinity)
        }
        .task(id: model.cliPath) { cliVersion = try? await model.client?.version() }
    }

    // MARK: Hero

    private var hero: some View {
        HStack(spacing: 18) {
            AppIconTile(size: 88)
            VStack(alignment: .leading, spacing: 6) {
                Text("cc-moss")
                    .font(.system(size: 26, weight: .semibold))
                Text(L.t("Civilizations carry on. So do conversations.", "文明会延续，对话也是。"))
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                HStack(spacing: 6) {
                    VersionChip(label: "App", value: model.version)
                    VersionChip(label: L.t("CLI", "命令行"), value: cliVersion ?? "—")
                    if let cli = cliVersion { alignButton(cli: cli) }
                }
                .padding(.top, 2)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12)
        // Room for the icon's shadow inside the scroll area, which clips.
        .padding(.top, 16)
        .padding(.bottom, 4)
    }

    // MARK: Versions

    /// When the app and the command differ, one button brings the older one
    /// up to date (A12): `cc-moss update` or, for the app, `cc-moss install`.
    @ViewBuilder private func alignButton(cli: String) -> some View {
        switch VersionMatch.of(app: model.version, cli: cli) {
        case .cliOlder:
            SoftButton(title: aligning ? L.t("Updating…", "正在升级…") : L.t("Update Command Line", "升级命令行"),
                       icon: "exclamationmark.triangle.fill") { align(.cliOlder) }
                .disabled(aligning)
                .tip(L.t("The command line is older than the app. This runs cc-moss update.",
                         "命令行比 App 旧，点一下运行 cc-moss update。"))
        case .appOlder:
            SoftButton(title: aligning ? L.t("Updating…", "正在更新…") : L.t("Update the App", "更新 App"),
                       icon: "exclamationmark.triangle.fill") { align(.appOlder) }
                .disabled(aligning)
                .tip(L.t("The app is older than the command line. It updates itself and reopens.",
                         "App 比命令行旧，点一下就换成新版，并自动重新打开。"))
        case .same, .unknown:
            EmptyView()
        }
    }

    private func align(_ match: VersionMatch) {
        guard let client = model.client, !aligning else { return }
        aligning = true
        Task {
            do {
                if match == .cliOlder { try await client.updateCLI() } else { try await client.installApp() }
                problem = nil
            } catch {
                problem = L.t("Didn't finish: \(error)", "没弄完：\(error)")
            }
            aligning = false
            cliVersion = try? await client.version()
        }
    }

    // MARK: Update

    private var updateTitle: String {
        switch update {
        case .idle: L.t("Check for a new version", "看看有没有新版本")
        case .checking: L.t("Checking…", "正在检查…")
        case .current: L.t("You're up to date", "已经是最新版本")
        case .available(let info): L.t("cc-moss \(info.latest) is out", "cc-moss \(info.latest) 发布了")
        case .failed: L.t("Couldn't check", "这次没查成")
        }
    }

    private var updateDetail: String? {
        if let problem { return problem }
        switch update {
        case .idle: return L.t("It also checks once a day on its own.", "它每天也会自己查一次。")
        case .checking: return nil
        case .current(let v): return L.t("\(v) is the latest release.", "\(v) 就是最新的。")
        case .available(let info):
            return L.t("You have \(info.current). It updates in your terminal, so you can watch it.",
                       "你现在是 \(info.current)。会在终端里升级，过程看得见。")
        case .failed(let why): return why
        }
    }

    @ViewBuilder private var updateControl: some View {
        switch update {
        case .checking:
            ProgressView().controlSize(.small)
        case .available(let info):
            HStack(spacing: 8) {
                if let notes = info.notes, let url = URL(string: notes) {
                    SoftButton(title: L.t("What's New", "更新了什么"), icon: "sparkles") { NSWorkspace.shared.open(url) }
                }
                SoftButton(title: L.t("Update in Terminal", "在终端里升级"), icon: Icons.update, prominent: true) {
                    runUpdate()
                }
            }
        default:
            SoftButton(title: L.t("Check Now", "立即检查"), icon: Icons.refresh) { Task { await check() } }
                .disabled(model.client == nil)
        }
    }

    private func check() async {
        guard let client = model.client else { return }
        problem = nil
        update = .checking
        do {
            let info = try await client.updateInfo()
            update = info.available ? .available(info) : .current(info.current)
        } catch MossClient.Failure.command(let message) {
            update = .failed(message)
        } catch {
            update = .failed(L.t("Couldn't reach GitHub. Try again in a moment.", "连不上 GitHub，过会儿再试。"))
        }
    }

    private func runUpdate() {
        guard let cli = model.cliPath else { return }
        let command = LaunchPlan.cliCommand(cli: cli, ["update"])
        if let failure = Opener.run(command: command, in: model.terminals.current) {
            model.copy(failure.command)
            problem = L.t("Couldn't open \(failure.terminalName). The command is on your clipboard: paste it in a terminal.",
                          "没能打开\(failure.terminalName)。命令已经复制好了，贴到终端里运行就行。")
        }
    }
}

/// `App 0.1.1`: one version in a small capsule.
struct VersionChip: View {
    let label: String
    let value: String

    var body: some View {
        HStack(spacing: 4) {
            Text(label).foregroundStyle(.secondary)
            Text(value).fontWeight(.semibold).monospacedDigit()
        }
        .font(.system(size: 11))
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(Capsule().fill(Color.primary.opacity(0.06)))
    }
}

/// A row that opens a web page: icon, title, and an arrow pointing out.
struct LinkRow: View {
    let icon: String
    let title: String
    var url = ""
    var divider = true
    /// Something in the app instead of a web page (no outward arrow).
    var perform: (() -> Void)?
    @State private var hovering = false

    var body: some View {
        Button {
            if let perform {
                perform()
            } else if let u = URL(string: url) {
                NSWorkspace.shared.open(u)
            }
        } label: {
            VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Image(systemName: icon)
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                        .frame(width: 20)
                    Text(title).font(.system(size: 14))
                    Spacer()
                    Image(systemName: perform == nil ? "arrow.up.right" : "chevron.right")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(hovering ? .secondary : .tertiary)
                }
                .padding(.horizontal, 12)
                .frame(minHeight: 44)
                .background(RoundedRectangle(cornerRadius: 10).fill(hovering ? Color.primary.opacity(0.04) : .clear))
                if divider {
                    Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }
}

enum SettingsWindow {
    /// A menu bar app is never the active app, so a window it opens lands
    /// behind whatever you were using. SwiftUI creates the window a moment
    /// after `openWindow`, so look for it a few times, then raise it.
    @MainActor static func bringToFront(attempt: Int = 0) {
        let window = NSApp.windows.first { $0.identifier?.rawValue.hasPrefix("settings") == true }
        guard let window, window.isVisible || attempt > 3 else {
            if attempt < 20 {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { bringToFront(attempt: attempt + 1) }
            }
            return
        }
        NSApp.activate(ignoringOtherApps: true)
        fitContent(of: window)
        tuckWindowButtons(in: window)
        window.makeKeyAndOrderFront(nil)
        window.orderFrontRegardless()
    }

    /// A hidden title bar still keeps its 28 pt in the window's layout, and
    /// the view was drawn into it, leaving a strip at the bottom. Let the
    /// content own the whole window and make the window exactly its size.
    @MainActor static func fitContent(of window: NSWindow) {
        window.styleMask.insert(.fullSizeContentView)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.setContentSize(NSSize(width: SettingsView.size.width, height: SettingsView.size.height))
    }

    /// Move the close / minimise / zoom buttons inside the floating sidebar,
    /// as Raycast does, instead of on its rounded corner.
    /// The window buttons sit inside the sidebar the way System Settings'
    /// do: an empty unified toolbar makes the title bar tall, and macOS
    /// itself centres the buttons in it. Moving them by hand didn't last:
    /// macOS lays the title bar out again on focus, resize and appearance
    /// changes and put them back.
    @MainActor static func tuckWindowButtons(in window: NSWindow) {
        guard window.toolbar == nil else { return }
        let toolbar = NSToolbar(identifier: "cc-moss.settings")
        toolbar.showsBaselineSeparator = false
        toolbar.displayMode = .iconOnly
        window.toolbar = toolbar
        window.toolbarStyle = .unified
        window.titlebarSeparatorStyle = .none
    }
}

/// An invisible view that re-applies the settings window's chrome after
/// each layout of the window: full-size content, our size, buttons tucked in.
struct WindowLayoutHook: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { HookView() }
    func updateNSView(_ nsView: NSView, context: Context) {}

    final class HookView: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            apply()
        }

        override func layout() {
            super.layout()
            apply()
        }

        private func apply() {
            guard let window else { return }
            MainActor.assumeIsolated {
                if !window.styleMask.contains(.fullSizeContentView) || window.titleVisibility != .hidden {
                    SettingsWindow.fitContent(of: window)
                }
                SettingsWindow.tuckWindowButtons(in: window)
            }
        }
    }
}
