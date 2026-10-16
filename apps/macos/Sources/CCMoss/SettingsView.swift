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
    @State private var search = ""
    @State private var back: [Tab] = []
    @State private var forward: [Tab] = []
    @Environment(\.colorScheme) private var scheme

    static let size = CGSize(width: 820, height: 560)

    enum Tab: CaseIterable, Hashable {
        case general, about

        var title: String {
            switch self {
            case .general: L.t("General", "通用")
            case .about: L.t("About", "关于")
            }
        }

        var icon: String {
            switch self {
            case .general: Icons.general
            case .about: "infinity"
            }
        }

        var tint: [Color] {
            switch self {
            case .general: [Color(white: 0.62), Color(white: 0.45)]
            case .about: [Theme.brand.opacity(0.85), Theme.brand]
            }
        }

        /// Words that find this section from the search field.
        var keywords: [String] {
            switch self {
            case .general: [title, "cc-moss", L.t("login startup command path version update", "开机 登录 启动 命令 位置 路径 版本 更新")]
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
        .frame(width: Self.size.width, height: Self.size.height)
        .background(Theme.surface(scheme))
        .background(.regularMaterial)
        .ignoresSafeArea()
        .focusEffectDisabled()
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
    @State private var found: String?
    @State private var checking = false
    @State private var updateMessage: String?

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
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: openAtLogin) { _, on in setOpenAtLogin(on) }
            }
        }
        MenuBarSettings(model: model)
        SettingsCard(title: "cc-moss") {
            CardRow(
                title: L.t("Program", "程序位置"),
                detail: model.cliPath.map { Grouping.tilde($0, home: NSHomeDirectory()) }
                    ?? L.t("Not found in the usual places", "常见位置里没找到")
            ) {
                HStack(spacing: 8) {
                    if UserDefaults.standard.string(forKey: "cliPath") != nil {
                        SoftButton(title: L.t("Automatic", "自动"), icon: "arrow.uturn.backward") { set(nil) }
                            .help(L.t("Find it in the usual places again", "重新在常见位置自动找"))
                    }
                    SoftButton(title: L.t("Choose…", "选择…"), icon: Icons.choose) { choose() }
                }
            }
            CardRow(
                title: L.t("Version", "版本"),
                detail: updateMessage,
                divider: false
            ) {
                HStack(spacing: 10) {
                    HStack(spacing: 5) {
                        Circle().fill(found == nil ? Theme.amber : .green).frame(width: 7, height: 7)
                        Text(found ?? "—")
                            .font(.system(size: 12, weight: .medium, design: .rounded))
                            .foregroundStyle(.secondary)
                    }
                    if checking {
                        ProgressView().controlSize(.small)
                    } else {
                        SoftButton(title: L.t("Check for Updates", "检查更新"), icon: Icons.update) {
                            Task { await checkUpdate() }
                        }
                        .disabled(model.client == nil)
                    }
                }
            }
        }
        .task(id: model.cliPath) { found = try? await model.client?.version() }
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

    private func checkUpdate() async {
        guard let client = model.client else { return }
        checking = true
        defer { checking = false }
        do {
            updateMessage = try await client.updateCheck()
        } catch MossClient.Failure.command(let message) {
            updateMessage = message
        } catch {
            updateMessage = L.t("Couldn't check right now.", "暂时查不了，过会儿再试。")
        }
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

struct AboutSettings: View {
    @Bindable var model: AppModel

    var body: some View {
        VStack(spacing: 10) {
            AppIconTile(size: 104)
                .padding(.top, 20)
                .padding(.bottom, 10)
            Text("cc-moss")
                .font(.system(size: 24, weight: .semibold))
            Text(L.t("Version \(model.version)", "版本 \(model.version)"))
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
            Text(L.t("Civilizations carry on. So do conversations.", "文明会延续，对话也是。"))
                .font(.system(size: 14))
                .foregroundStyle(.secondary)
                .padding(.top, 6)
            HStack(spacing: 10) {
                SoftButton(title: "GitHub", icon: Icons.github) {
                    NSWorkspace.shared.open(URL(string: "https://github.com/larrykoo711/cc-moss")!)
                }
                SoftButton(title: L.t("Release Notes", "更新记录"), icon: "doc.text") {
                    NSWorkspace.shared.open(URL(string: "https://github.com/larrykoo711/cc-moss/blob/main/CHANGELOG.md")!)
                }
            }
            .padding(.top, 14)
            Text(L.t("MIT License. Not affiliated with Anthropic.", "MIT 开源。与 Anthropic 没有关联。"))
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
                .padding(.top, 18)
        }
        .frame(maxWidth: .infinity)
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
