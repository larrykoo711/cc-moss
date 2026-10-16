import AppKit
import MossKit
import SwiftUI

/// The first-run welcome (A22, T4.2): six short steps that each do their
/// job on the spot, in the panel's look. Steps slide left and right (a
/// cross-fade with Reduce Motion); any step, or the whole thing, can be skipped.
struct WelcomeView: View {
    @Bindable var model: AppModel
    /// False for screenshots: nothing is asked of the CLI or the system.
    var live = true
    var finish: (_ openPanel: Bool) -> Void = { _ in }

    @State private var flow: WelcomeFlow
    @State private var forward = true
    @State private var plan: InstallPlan?
    @State private var planProblem: String?
    @State private var installing = false
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    static let size = CGSize(width: 640, height: 500)

    init(model: AppModel, start: WelcomeFlow.Step = .welcome, plan: InstallPlan? = nil, live: Bool = true,
         finish: @escaping (_ openPanel: Bool) -> Void = { _ in }) {
        self.model = model
        self.live = live
        self.finish = finish
        var flow = WelcomeFlow()
        while flow.step != start { flow.next() }
        _flow = State(initialValue: flow)
        _plan = State(initialValue: plan)
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Spacer()
                Button(L.t("Skip Welcome", "跳过引导")) { finish(false) }
                    .buttonStyle(.plain)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .handCursor()
            }
            .padding(.top, 14)
            .padding(.horizontal, 20)
            page
                .id(flow.step)
                .transition(transition)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            footer
        }
        .frame(width: Self.size.width, height: Self.size.height)
        .background(Theme.surface(scheme))
        .background(.regularMaterial)
        .ignoresSafeArea()
        .tooltipLayer()
        .task { if live { await refreshPlan() } }
        .onChange(of: NotificationStatus.shared.auth) { _, _ in tick() }
    }

    private var transition: AnyTransition {
        if reduceMotion { return .opacity }
        return .asymmetric(
            insertion: .move(edge: forward ? .trailing : .leading).combined(with: .opacity),
            removal: .move(edge: forward ? .leading : .trailing).combined(with: .opacity)
        )
    }

    // MARK: - Pages

    @ViewBuilder private var page: some View {
        switch flow.step {
        case .welcome:
            WelcomePage(art: AnyView(LayerPet(pose: .happy, size: 76, dark: scheme == .dark)),
                        title: L.t("Welcome to cc-moss", "欢迎使用 cc-moss"),
                        subtitle: L.t("Civilizations carry on. So do conversations.", "文明会延续，对话也是。")) {
                VStack(spacing: 10) {
                    FeatureRow(icon: "eye", tint: SettingsView.Tab.menuBar.tint,
                               title: L.t("An eye on every session", "替你看着每个会话"),
                               detail: L.t("Busy, waiting for you or stopped: the menu bar and Mo tell you.",
                                           "在忙、等你、断了，菜单栏和小莫都会告诉你。"))
                    FeatureRow(icon: "arrow.counterclockwise", tint: SettingsView.Tab.about.tint,
                               title: L.t("Stopped? Bring it back", "断了也能找回"),
                               detail: L.t("After a restart or a crash, one click and the conversation carries on.",
                                           "重启或崩溃之后，点一下就接着聊。"))
                    FeatureRow(icon: "command", tint: SettingsView.Tab.aliases.tint,
                               title: L.t("There when you call", "随叫随到"),
                               detail: L.t("\(hotKeyText) opens the panel from anywhere.", "在哪儿按 \(hotKeyText) 都能打开面板。"))
                }
            }
        case .tools:
            WelcomePage(art: AnyView(IconTile(symbol: "terminal", colors: SettingsView.Tab.general.tint, size: 64)),
                        title: L.t("Set up the terminal side", "装好终端这一侧"),
                        subtitle: L.t("Hooks tell cc-moss what each session is doing; tmux is what lets a stopped one come back.",
                                      "钩子让 cc-moss 知道会话在做什么，tmux 让断了的会话能找回来。")) {
                toolsBody
            }
        case .startup:
            WelcomePage(art: AnyView(IconTile(symbol: "bell.badge", colors: SettingsView.Tab.notifications.tint, size: 64)),
                        title: L.t("Here from the start", "开机就在，有事就说"),
                        subtitle: L.t("So a restart never goes unnoticed.", "这样重启之后，断了的会话也不会被忘记。")) {
                StartupBody(live: live)
            }
        case .terminal:
            WelcomePage(art: AnyView(IconTile(symbol: "macwindow", colors: SettingsView.Tab.menuBar.tint, size: 64)),
                        title: L.t("Where sessions open", "会话在哪儿打开"),
                        subtitle: L.t("Pick the terminal you live in. You can change it later in Settings.",
                                      "选你常用的终端，之后在设置里也能改。")) {
                SettingsCard {
                    CardRow(title: L.t("Terminal", "终端"),
                            detail: L.t("Ghostty is fast, native and free.", "推荐 Ghostty：快、原生、免费。"),
                            divider: false) {
                        TerminalPicker(prefs: model.terminals)
                    }
                }
            }
        case .mo:
            MoBody()
        case .done:
            WelcomePage(art: AnyView(AppIconTile(size: 72)),
                        title: L.t("You're all set", "准备好了"),
                        subtitle: L.t("Press this whenever you want the panel.", "想看会话时，按下它。")) {
                HStack(spacing: 6) {
                    ForEach(Array(hotKeyText.map(String.init).enumerated()), id: \.offset) { _, key in
                        Text(key)
                            .font(.system(size: 20, weight: .medium))
                            .frame(minWidth: 44, minHeight: 44)
                            .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Theme.raised(scheme)))
                            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.hairline(scheme)))
                            .shadow(color: .black.opacity(0.08), radius: 2, y: 1)
                    }
                }
                .padding(.top, 6)
            }
        }
    }

    private var hotKeyText: String { HotKey.load(.standard)?.display ?? "⌃⌥M" }

    // MARK: - Terminal tools

    @ViewBuilder private var toolsBody: some View {
        VStack(spacing: 12) {
            SettingsCard {
                if let plan {
                    ForEach(Array(plan.visible.enumerated()), id: \.element.id) { i, part in
                        CardRow(title: Self.partTitle(part.part), detail: Self.partDetail(part.part),
                                divider: i < plan.visible.count - 1) {
                            PartChip(state: part.state)
                        }
                    }
                } else {
                    CardRow(title: planProblem ?? L.t("Looking…", "正在看…"), divider: false) {
                        if planProblem == nil { ProgressView().controlSize(.small) }
                    }
                }
            }
            HStack(spacing: 8) {
                if let plan, plan.isReady {
                    Label(L.t("All set here", "这一侧都好了"), systemImage: "checkmark.circle.fill")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.green)
                } else {
                    Text(L.t("Same as running cc-moss install; cc-moss install --uninstall undoes it.",
                             "和运行 cc-moss install 一样；cc-moss install --uninstall 可以撤销。"))
                        .font(.system(size: 11))
                        .foregroundStyle(Theme.caption)
                }
                Spacer()
                if installing {
                    ProgressView().controlSize(.small)
                } else if let plan, !plan.isReady, model.client != nil {
                    SoftButton(title: L.t("Set It Up", "一键装好"), icon: "wand.and.stars", prominent: true) { install() }
                }
            }
            .padding(.horizontal, 4)
        }
    }

    static func partTitle(_ part: String) -> String {
        switch part {
        case "hooks": L.t("Claude Code hooks", "Claude Code 钩子")
        case "tmux": "tmux"
        case "starship": L.t("starship prompt", "starship 提示符")
        default: L.t("Shell setup", "shell 配置")
        }
    }

    static func partDetail(_ part: String) -> String {
        switch part {
        case "hooks": L.t("How cc-moss hears what sessions do", "cc-moss 靠它知道会话在做什么")
        case "tmux": L.t("Keeps sessions so they can come back", "让会话断了也能找回")
        case "starship": L.t("The session's name in your prompt", "在提示符里看到会话名")
        default: L.t("The t picker and friends", "t 这样的快捷命令")
        }
    }

    private func refreshPlan() async {
        guard let client = model.client else {
            if plan == nil { planProblem = L.t("cc-moss isn't installed yet", "还没装 cc-moss 命令") }
            return
        }
        do {
            plan = try await client.installPlan()
            planProblem = nil
        } catch {
            if plan == nil { planProblem = L.t("Couldn't look: \(error)", "没看成：\(error)") }
        }
        tick()
    }

    private func install() {
        guard let client = model.client else { return }
        installing = true
        Task {
            do {
                try await client.installTools()
            } catch {
                planProblem = L.t("Didn't finish: \(error)", "没装完：\(error)")
            }
            installing = false
            await refreshPlan()
        }
    }

    /// Tick what the machine already satisfies.
    private func tick() {
        var done: Set<WelcomeFlow.Step> = []
        if plan?.isReady == true { done.insert(.tools) }
        if LoginItem.isEnabled, NotificationStatus.shared.auth == .allowed { done.insert(.startup) }
        flow.satisfied(done)
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 10) {
            StepDots(flow: flow)
            Spacer()
            if flow.step != .done, flow.step != .welcome {
                Button(L.t("Skip This Step", "这步先跳过")) { go { $0.skipStep() } }
                    .buttonStyle(.plain)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .handCursor()
                    .padding(.trailing, 4)
            }
            if flow.canGoBack {
                SoftButton(title: L.t("Back", "上一步"), icon: "chevron.left") {
                    forward = false
                    withAnimation(.spring(response: 0.35, dampingFraction: 0.86)) { flow.back() }
                }
            }
            SoftButton(title: flow.step == .done ? L.t("Start", "开始使用") : L.t("Continue", "继续"),
                       icon: flow.step == .done ? "checkmark" : "arrow.right", prominent: true, shortcut: "↵") {
                if flow.step == .done { finish(true) } else { go { $0.next() } }
            }
            .keyboardShortcut(.defaultAction)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 16)
    }

    private func go(_ change: (inout WelcomeFlow) -> Void) {
        forward = true
        withAnimation(.spring(response: 0.35, dampingFraction: 0.86)) { change(&flow) }
    }
}

/// One step: art, a title, a sentence, then what the step does.
private struct WelcomePage<Content: View>: View {
    let art: AnyView
    let title: String
    let subtitle: String
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(spacing: 10) {
            art.frame(height: 96)
            Text(title)
                .font(.system(size: 22, weight: .semibold))
            Text(subtitle)
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: 440)
            VStack(alignment: .leading, spacing: 16) { content() }
                .frame(width: 480)
                .padding(.top, 10)
        }
        .padding(.top, 4)
    }
}

/// A line of what cc-moss does, with its icon.
private struct FeatureRow: View {
    let icon: String
    let tint: [Color]
    let title: String
    let detail: String

    var body: some View {
        HStack(spacing: 12) {
            IconTile(symbol: icon, colors: tint, size: 30)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.system(size: 13, weight: .semibold))
                Text(detail).font(.system(size: 12)).foregroundStyle(Theme.caption)
            }
            Spacer(minLength: 0)
        }
    }
}

/// A part's state in the terminal tools step.
private struct PartChip: View {
    let state: InstallPlan.Part.State

    var body: some View {
        let (text, icon, tint): (String, String, Color) = switch state {
        case .ready: (L.t("Ready", "已就绪"), "checkmark.circle.fill", .green)
        case .changes: (L.t("To set up", "待安装"), "circle.dashed", Theme.amber)
        case .skipped: (L.t("Skipped", "跳过"), "minus.circle", .secondary)
        case .unsupported: (L.t("Not needed", "不需要"), "minus.circle", .secondary)
        }
        Label(text, systemImage: icon)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(tint)
            .labelStyle(.titleAndIcon)
    }
}

/// Open at login (on by default: this is the one time we ask, A8) and
/// notifications.
private struct StartupBody: View {
    let live: Bool
    @State private var openAtLogin = true
    @State private var problem: String?
    private var status: NotificationStatus { NotificationStatus.shared }

    var body: some View {
        SettingsCard {
            CardRow(title: L.t("Open at Login", "开机自动打开"),
                    detail: problem ?? L.t("Starts quietly in the menu bar.", "悄悄待在菜单栏里。")) {
                Toggle("", isOn: $openAtLogin)
                    .toggleStyle(.switch)
                    .cursor(.pointingHand)
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: openAtLogin) { _, on in apply(on) }
            }
            CardRow(title: L.t("Notifications", "通知"),
                    detail: L.t("When a session needs you or stops.", "会话需要你、或者断了的时候。"),
                    divider: false) {
                if status.auth == .allowed {
                    Label(L.t("Allowed", "已允许"), systemImage: "checkmark.circle.fill")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.green)
                } else if status.auth == .denied {
                    SoftButton(title: L.t("Open System Settings", "打开系统设置"), icon: "gearshape") {
                        NotificationStatus.openSystemSettings()
                    }
                } else {
                    SoftButton(title: L.t("Allow", "允许通知"), icon: "bell") { Task { await status.request() } }
                }
            }
        }
        .onAppear {
            guard live else { return }
            // On unless already decided off: turning it on is this step's suggestion.
            if !LoginItem.isEnabled { apply(true) }
            openAtLogin = LoginItem.isEnabled
            Task { await status.refresh() }
        }
    }

    private func apply(_ on: Bool) {
        do {
            try LoginItem.set(on)
            problem = nil
        } catch {
            problem = L.t("Couldn't change it: \(error.localizedDescription)", "没改成：\(error.localizedDescription)")
        }
        openAtLogin = LoginItem.isEnabled
    }
}

/// Mo, and whether it speaks (asked here, DESIGN-pet §6.4).
private struct MoBody: View {
    @AppStorage(PetController.enabledKey) private var enabled = false
    @AppStorage(VoiceMode.key) private var voice = VoiceMode.off.rawValue
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        WelcomePage(art: AnyView(LayerPet(pose: enabled ? .happy : .sleeping, size: 76, dark: scheme == .dark)),
                    title: L.t("Shall Mo keep you company?", "要不要小莫陪着你？"),
                    subtitle: L.t("A small companion in a corner of your desktop. Quiet until something needs you.",
                                  "桌面一角的小伙伴，平时安安静静，有事才找你。")) {
            SettingsCard {
                CardRow(title: L.t("Show Mo", "显示小莫"), divider: enabled) {
                    Toggle("", isOn: $enabled.animation(.easeOut(duration: 0.2)))
                        .toggleStyle(.switch)
                        .cursor(.pointingHand)
                        .labelsHidden()
                        .controlSize(.small)
                        .onChange(of: enabled) { _, _ in
                            NotificationCenter.default.post(name: PetController.settingsChanged, object: nil)
                        }
                }
                if enabled {
                    CardRow(title: L.t("Speak Up", "开口说话"),
                            detail: L.t("Off by default. It reads only the message title.", "默认不出声；开口时只念消息标题。"),
                            divider: false) {
                        SlidingPicker(selection: $voice, options: [
                            (VoiceMode.off.rawValue, L.t("Off", "关")),
                            (VoiceMode.chime.rawValue, L.t("Chime", "提示音")),
                            (VoiceMode.speak.rawValue, L.t("Speak", "说话")),
                        ])
                        .onChange(of: voice) { _, _ in
                            NotificationCenter.default.post(name: PetController.settingsChanged, object: nil)
                        }
                    }
                    .transition(.opacity)
                }
            }
        }
    }
}

/// The dots under a step: the current one long, done ones in colour.
private struct StepDots: View {
    let flow: WelcomeFlow

    var body: some View {
        HStack(spacing: 6) {
            ForEach(WelcomeFlow.Step.allCases, id: \.self) { s in
                Capsule()
                    .fill(fill(s))
                    .frame(width: s == flow.step ? 18 : 6, height: 6)
            }
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.8), value: flow.step)
        .accessibilityLabel(L.t("Step \(flow.progress.0) of \(flow.progress.1)", "第 \(flow.progress.0) 步，共 \(flow.progress.1) 步"))
    }

    private func fill(_ s: WelcomeFlow.Step) -> Color {
        if s == flow.step { return Theme.brand }
        if flow.state(of: s) == .done { return Theme.brand.opacity(0.4) }
        return Color.primary.opacity(0.15)
    }
}

/// The welcome's window: shown once at first launch, or from Settings › About.
@MainActor
enum WelcomeWindow {
    private static var window: NSWindow?

    /// What "Start" opens; set by the app delegate.
    static var openPanel: () -> Void = {}

    static func show(model: AppModel) {
        if let window {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: WelcomeView.size),
                         styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: false)
        w.titlebarAppearsTransparent = true
        w.titleVisibility = .hidden
        w.isMovableByWindowBackground = true
        w.isReleasedWhenClosed = false
        w.title = L.t("Welcome to cc-moss", "欢迎使用 cc-moss")
        w.standardWindowButton(.miniaturizeButton)?.isHidden = true
        w.standardWindowButton(.zoomButton)?.isHidden = true
        w.contentView = NSHostingView(rootView: WelcomeView(model: model) { open in
            close()
            if open { openPanel() }
        }.followsLanguage())
        w.center()
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: w, queue: .main) { _ in
            MainActor.assumeIsolated {
                // Closed, skipped or finished: either way, not again at launch.
                WelcomeFlow.markSeen(.standard)
                window = nil
            }
        }
        window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    static func close() {
        window?.close()
    }
}
