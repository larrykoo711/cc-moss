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
    @State private var search = ""
    @State private var back: [Tab] = []
    @State private var forward: [Tab] = []
    @Environment(\.colorScheme) private var scheme

    static let size = CGSize(width: 820, height: 560)

    enum Tab: CaseIterable, Hashable {
        case general, aliases, about

        var title: String {
            switch self {
            case .general: L.t("General", "通用")
            case .aliases: L.t("Aliases", "别名")
            case .about: L.t("About", "关于")
            }
        }

        var icon: String {
            switch self {
            case .general: Icons.general
            case .aliases: Icons.aliases
            case .about: "infinity"
            }
        }

        var tint: [Color] {
            switch self {
            case .general: [Color(white: 0.62), Color(white: 0.45)]
            case .aliases: AliasRow.tint
            case .about: [Theme.brand.opacity(0.85), Theme.brand]
            }
        }

        /// Words that find this section from the search field.
        var keywords: [String] {
            switch self {
            case .general: [title, "cc-moss", L.t("login startup command path version update terminal ghostty iterm wezterm kitty alacritty", "开机 登录 启动 命令 位置 路径 版本 更新 终端 ghostty iterm wezterm kitty alacritty")]
            case .aliases: [title, "alias t", L.t("shortcut command shell link", "快捷 命令 shell 链接 alias")]
            case .about: [title, "cc-moss", "GitHub", L.t("version license", "版本 许可")]
            }
        }
    }

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
                NavCapsule(
                    canGoBack: !back.isEmpty, canGoForward: !forward.isEmpty,
                    goBack: { if let t = back.popLast() { forward.append(tab); tab = t } },
                    goForward: { if let t = forward.popLast() { back.append(tab); tab = t } }
                )
                .padding(.top, 10)
                .padding(.bottom, 12)
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        switch tab {
                        case .general: GeneralSettings(model: model)
                        case .aliases: AliasesSettings(model: model, demo: aliasDemo)
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
        .onAppear { SettingsWindow.bringToFront() }
        .onChange(of: search) { _, _ in
            if let first = tabs.first, !tabs.contains(tab) { go(first) }
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 2) {
            Spacer().frame(height: 40) // the window buttons sit here
            // Raycast's large rounded search field.
            HStack(spacing: 8) {
                Image(systemName: Icons.search)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(.secondary)
                TextField(L.t("Search settings…", "搜索设置…"), text: $search)
                    .textFieldStyle(.plain)
                    .font(.system(size: 14))
            }
            .padding(.horizontal, 12)
            .frame(height: 36)
            .background(Capsule().fill(Color.primary.opacity(scheme == .dark ? 0.08 : 0.06)))
            .padding(.bottom, 10)
            ForEach(tabs, id: \.self) { t in
                SidebarItem(tab: t, selected: t == tab) { go(t) }
            }
            Spacer()
        }
        .padding(.horizontal, 10)
        .padding(.bottom, 10)
        .frame(maxHeight: .infinity)
        .glassPanel(cornerRadius: 18)
    }
}

/// A sidebar entry: an icon tile and the section's name, Raycast-sized.
struct SidebarItem: View {
    let tab: SettingsView.Tab
    let selected: Bool
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                IconTile(symbol: tab.icon, colors: tab.tint, size: 22)
                Text(tab.title).font(.system(size: 14))
                Spacer(minLength: 0)
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
                .help(L.t("Back", "后退"))
            Rectangle().fill(Color.primary.opacity(0.12)).frame(width: 1, height: 16)
            arrow("chevron.right", enabled: canGoForward, action: goForward)
                .help(L.t("Forward", "前进"))
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
                    .font(.system(size: 14, weight: .semibold))
                    .frame(minHeight: 32, alignment: .center)
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
                            .foregroundStyle(.secondary)
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
    var onSubmit: () -> Void = {}
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        TextField(placeholder, text: $text)
            .textFieldStyle(.plain)
            .font(.system(size: 12, design: monospaced ? .monospaced : .default))
            .focused($focused)
            .onSubmit(onSubmit)
            .padding(.horizontal, 10)
            .frame(height: 28)
            .background(RoundedRectangle(cornerRadius: 7).fill(Theme.selection(scheme).opacity(0.8)))
            .overlay(
                RoundedRectangle(cornerRadius: 7)
                    .strokeBorder(focused ? Theme.brand.opacity(0.7) : Theme.hairline(scheme), lineWidth: focused ? 1.5 : 1)
            )
            .animation(.easeOut(duration: 0.12), value: focused)
    }
}

/// A small rounded button with hover and press feedback.
struct SoftButton: View {
    let title: String
    let icon: String
    var prominent = false
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: icon).font(.system(size: 11, weight: .medium))
                Text(title).font(.system(size: 12, weight: .medium)).lineLimit(1).fixedSize()
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

struct GeneralSettings: View {
    @Bindable var model: AppModel

    @State private var openAtLogin = SMAppService.mainApp.status == .enabled
    @State private var loginError: String?

    var body: some View {
        SettingsCard {
            CardRow(
                title: L.t("Open at Login", "开机自动打开"),
                detail: loginError ?? L.t("Keep an eye on your sessions from the moment you log in.",
                                         "一登录就开始替你看着会话。"),
                divider: false
            ) {
                Toggle("", isOn: $openAtLogin)
                    .toggleStyle(.switch)
                    .handCursor()
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: openAtLogin) { _, on in setOpenAtLogin(on) }
            }
        }
        TerminalSettings(prefs: model.terminals)
        MenuBarSettings(model: model)
        SettingsCard(title: "cc-moss") {
            CardRow(
                title: L.t("Program", "程序位置"),
                detail: model.cliPath.map { Grouping.tilde($0, home: NSHomeDirectory()) }
                    ?? L.t("Not found in the usual places", "常见位置里没找到"),
                divider: false
            ) {
                HStack(spacing: 8) {
                    if UserDefaults.standard.string(forKey: "cliPath") != nil {
                        SoftButton(title: L.t("Automatic", "自动"), icon: "arrow.uturn.backward") { set(nil) }
                            .help(L.t("Find it in the usual places again", "重新在常见位置自动找"))
                    }
                    SoftButton(title: L.t("Choose…", "选择…"), icon: Icons.choose) { choose() }
                }
            }
        }
    }

    private func setOpenAtLogin(_ on: Bool) {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            loginError = nil
        } catch {
            loginError = L.t("macOS said no: \(error.localizedDescription)", "macOS 没同意：\(error.localizedDescription)")
            openAtLogin = SMAppService.mainApp.status == .enabled
        }
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
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.message = L.t("Choose the cc-moss program", "选择 cc-moss 程序")
        if panel.runModal() == .OK, let url = panel.url { set(url.path) }
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
                CardRow(title: updateTitle, detail: updateDetail, divider: false) {
                    updateControl
                }
            }
            SettingsCard(title: L.t("Learn More", "了解更多")) {
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
                    if let cli = cliVersion, cli != model.version, model.version != "dev" {
                        Label(L.t("Versions differ", "版本不一致"), systemImage: "exclamationmark.triangle.fill")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(Theme.amber)
                            .help(L.t("The app and the cc-moss command should be the same version. Updating fixes it.",
                                      "App 和 cc-moss 命令应该是同一个版本，升级一下就好。"))
                    }
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
    let url: String
    var divider = true
    @State private var hovering = false

    var body: some View {
        Button {
            if let u = URL(string: url) { NSWorkspace.shared.open(u) }
        } label: {
            VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Image(systemName: icon)
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                        .frame(width: 20)
                    Text(title).font(.system(size: 14))
                    Spacer()
                    Image(systemName: "arrow.up.right")
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
        .help(url)
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
    @MainActor static func tuckWindowButtons(in window: NSWindow) {
        let kinds: [NSWindow.ButtonType] = [.closeButton, .miniaturizeButton, .zoomButton]
        for (i, kind) in kinds.enumerated() {
            guard let button = window.standardWindowButton(kind), let bar = button.superview else { continue }
            let step: CGFloat = 20
            button.setFrameOrigin(NSPoint(x: 22 + CGFloat(i) * step, y: bar.bounds.height - button.frame.height - 14))
        }
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
