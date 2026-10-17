import AppKit
import MossKit
import SwiftUI

/// The first-run welcome (A22, T4.2), after Raycast's: words and choices on
/// the left, a small animated stage on the right. Seven short steps, each
/// doing its job on the spot, in plain words: no commands, no tool names.
/// Every step change plays: the stage cross-fades and scales in, the words
/// rise in one after another from the side you're going. Soft system
/// sounds mark progress (silent when interface sounds are off).
struct WelcomeView: View {
    @Bindable var model: AppModel
    /// False for screenshots: nothing is asked of the CLI or the system.
    var live = true
    var finish: (_ openPanel: Bool) -> Void = { _ in }

    @State private var flow: WelcomeFlow
    @State private var forward = true
    /// Drives the staggered entrance; reset on every step change.
    @State private var shown = false
    @State private var appeared = false
    @State private var plan: InstallPlan?
    @State private var connecting = false
    @State private var connectProblem: String?
    /// Double Control pressed while on the shortcut step.
    @State private var shortcutHits = 0
    @State private var shortcut = PanelShortcut.load(.standard)
    @State private var asking = false
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    static let size = CGSize(width: 760, height: 520)

    init(model: AppModel, start: WelcomeFlow.Step = .welcome, plan: InstallPlan? = nil, live: Bool = true,
         finish: @escaping (_ openPanel: Bool) -> Void = { _ in }) {
        self.model = model
        self.live = live
        self.finish = finish
        var flow = WelcomeFlow()
        while flow.step != start { flow.next() }
        _flow = State(initialValue: flow)
        _plan = State(initialValue: plan)
        _shown = State(initialValue: !live)
        _appeared = State(initialValue: !live)
    }

    var body: some View {
        HStack(spacing: 0) {
            left
                .frame(width: 360)
            stage
                .padding(12)
        }
        // A language switch redraws every word in place: the step and what
        // you chose stay (the window doesn't rebuild the whole view).
        .id(LanguageState.shared.lang)
        .frame(width: Self.size.width, height: Self.size.height)
        .background(Theme.surface(scheme).opacity(1))
        .focusEffectDisabled()
        .scaleEffect(appeared ? 1 : 0.96)
        .opacity(appeared ? 1 : 0)
        .tooltipLayer()
        .task {
            guard live else { return }
            withAnimation(.spring(response: 0.45, dampingFraction: 0.85)) { appeared = true }
            enter()
            await refreshPlan()
        }
        .onChange(of: NotificationStatus.shared.auth) { _, new in
            if new == .allowed { WelcomeSound.success() }
            tick()
        }
        .onChange(of: HotKeyCenter.shared.trust.allowed) { _, allowed in
            guard allowed else { return }
            WelcomeSound.success()
            // Back from System Settings: bring the welcome forward again.
            WelcomeWindow.bringForward()
        }
        .onAppear { if live { WelcomeWindow.onShortcut = { triedIt() } } }
    }

    // MARK: - Left: words and choices

    private var left: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Continuum(phase: 0.25)
                    .stroke(style: StrokeStyle(lineWidth: 2.2, lineCap: .round, lineJoin: .round))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 20, height: 20)
                Text("cc-moss").font(.system(size: 13, weight: .semibold))
                Spacer()
                LanguageSwitch(client: model.client)
                    .padding(.trailing, 10)
                if flow.step != .done {
                    Button(L.t("Skip", "跳过")) { close(openPanel: false) }
                        .buttonStyle(.plain)
                        .focusable(false)
                        .font(.system(size: 12))
                        .foregroundStyle(.tertiary)
                        .handCursor()
                }
            }
            .padding(.top, 34)
            Spacer(minLength: 18)
            VStack(alignment: .leading, spacing: 12) {
                Text(copy.eyebrow)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                    .rise(shown, order: 0, forward: forward)
                Text(copy.title)
                    .font(.system(size: 26, weight: .bold))
                    .fixedSize(horizontal: false, vertical: true)
                    .rise(shown, order: 1, forward: forward)
                Text(copy.body)
                    .font(.system(size: 14))
                    .foregroundStyle(.secondary)
                    .lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
                    .rise(shown, order: 2, forward: forward)
                controls
                    .padding(.top, 8)
                    .rise(shown, order: 3, forward: forward)
            }
            .id(flow.step)
            Spacer(minLength: 18)
            footer
                .padding(.bottom, 26)
        }
        .padding(.horizontal, 32)
    }

    private struct Copy {
        var eyebrow: String
        var title: String
        var body: String
    }

    private var copy: Copy {
        let n = "\(flow.progress.0)/\(flow.progress.1)"
        switch flow.step {
        case .welcome:
            return Copy(eyebrow: L.t("Welcome", "欢迎"),
                        title: L.t("Every Claude Code session, looked after", "每个 Claude Code 会话，都有人照看"),
                        body: L.t("Who's busy, who's waiting for you, who got cut off: you'll know at a glance. And if your Mac restarts, everything comes right back.",
                                  "谁在忙、谁在等你、谁断了，一眼就知道。电脑重启了也不怕，点一下全都回来。"))
        case .tools:
            return Copy(eyebrow: n, title: L.t("Connect your terminal", "连上你的终端"),
                        body: L.t("One click, and cc-moss can hear what each session is up to.",
                                  "点一下，cc-moss 就能听见每个会话的动静。"))
        case .workspace:
            return Copy(eyebrow: n, title: L.t("Where do you work?", "你平时在哪儿干活？"),
                        body: L.t("Claude works inside a folder. Pick yours and new sessions start right there.",
                                  "Claude 在一个目录里干活。选好你的工作区，新会话就从那儿开始。"))
        case .shortcut:
            return Copy(eyebrow: n, title: L.t("There when you call", "随叫随到"),
                        body: L.t("In any app, \(shortcut.spoken) and your sessions are right there. Prefer other keys? Change it below.",
                                  "在任何 App 里\(shortcut.spoken)，会话就在眼前。想换别的键，下面点一下就行。"))
        case .startup:
            return Copy(eyebrow: n, title: L.t("We'll tap you on the shoulder", "有事，我们叫你"),
                        body: L.t("When a session needs you or stops, you'll hear about it, even right after a restart.",
                                  "会话需要你、或者断了的时候，第一时间告诉你，重启之后也一样。"))
        case .mo:
            return Copy(eyebrow: n, title: L.t("Shall Mo move in?", "请小莫搬到你桌面上？"),
                        body: L.t("It lives in a corner of your screen, stays quiet, and only perks up when something needs you.",
                                  "它住在屏幕一角，平时安安静静，有事才冒个泡。"))
        case .done:
            return Copy(eyebrow: L.t("All set", "就绪"), title: L.t("You're good to go", "一切就绪"),
                        body: L.t("\(shortcutText) opens your sessions. We'll take it from here.",
                                  "\(shortcutText) 打开会话面板。剩下的交给我们。"))
        }
    }

    private var shortcutText: String {
        let spoken = shortcut.spoken
        return spoken.prefix(1).uppercased() + spoken.dropFirst()
    }

    @ViewBuilder private var controls: some View {
        switch flow.step {
        case .welcome, .done:
            EmptyView()
        case .tools:
            connectControls
        case .workspace:
            VStack(spacing: 10) {
                ChoiceCard { WorkspaceChoice() }
                ChoiceCard { TerminalChoice(prefs: model.terminals) }
            }
        case .shortcut:
            ShortcutChoice(tried: shortcutHits > 0, shortcut: $shortcut)
        case .startup:
            StartupChoice(live: live)
        case .mo:
            MoChoice()
        }
    }

    // MARK: Connect

    private static let parts: [(id: String, icon: String, en: String, zh: String)] = [
        ("hooks", "waveform", "Hear what sessions are doing", "听见会话的动静"),
        ("tmux", "arrow.counterclockwise", "Bring stopped sessions back", "断了也能找回来"),
        ("starship", "tag", "Name each session in the terminal", "终端里认出每个会话"),
        ("shell", "command", "Type t to pick a session", "敲 t 就能挑会话"),
    ]

    private var connectControls: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(Self.parts.enumerated()), id: \.element.id) { i, part in
                let ready = plan?.parts.first { $0.part == part.id }?.state != .changes && plan != nil
                HStack(spacing: 10) {
                    ZStack {
                        Circle().fill(ready ? Color.green.opacity(0.15) : Color.primary.opacity(0.06))
                        if connecting, !ready {
                            ProgressView().controlSize(.mini)
                        } else {
                            Image(systemName: ready ? "checkmark" : part.icon)
                                .font(.system(size: 11, weight: .bold))
                                .foregroundStyle(ready ? Color.green : Color.secondary)
                        }
                    }
                    .frame(width: 26, height: 26)
                    .animation(.spring(response: 0.35, dampingFraction: 0.6).delay(Double(i) * 0.08), value: ready)
                    Text(L.t(part.en, part.zh)).font(.system(size: 13))
                }
            }
            if let connectProblem {
                Text(connectProblem).font(.system(size: 12)).foregroundStyle(Theme.amber)
            }
        }
    }

    private func refreshPlan() async {
        guard live, let client = model.client else { return }
        if let fresh = try? await client.installPlan() {
            withAnimation(.spring(response: 0.35, dampingFraction: 0.8)) { plan = fresh }
        }
        tick()
    }

    /// Connect, and on success move on by itself.
    private func connect() {
        guard let client = model.client else { return }
        connecting = true
        connectProblem = nil
        Task {
            do {
                try await client.installTools()
                WelcomeSound.success()
            } catch {
                connectProblem = L.t("That didn't finish. Try again in a moment.", "没连上，稍等再试一次。")
            }
            await refreshPlan()
            connecting = false
            if plan?.isReady == true {
                try? await Task.sleep(for: .milliseconds(650)) // let the ticks land
                if flow.step == .tools { move(forward: true) { $0.next() } }
            }
        }
    }

    /// The step's recommended thing, still to do: the main button does it.
    private enum Pending { case connect, notify, accessibility }

    private var pending: Pending? {
        guard live else { return nil }
        switch flow.step {
        case .tools:
            return plan?.isReady == true || model.client == nil ? nil : .connect
        case .startup:
            return [.allowed, .denied, .unavailable].contains(NotificationStatus.shared.auth) ? nil : .notify
        case .shortcut:
            return shortcut.tapKey != nil && !HotKeyCenter.shared.trust.allowed ? .accessibility : nil
        default:
            return nil
        }
    }

    private func doPending(_ p: Pending) {
        switch p {
        case .connect:
            connect()
        case .notify:
            asking = true
            Task {
                await NotificationStatus.shared.request()
                asking = false
                if flow.step == .startup { move(forward: true) { $0.next() } }
            }
        case .accessibility:
            HotKeyCenter.shared.askForAccessibility()
            HotKeyCenter.openAccessibilitySettings()
        }
    }

    private func tick() {
        var done: Set<WelcomeFlow.Step> = []
        if plan?.isReady == true { done.insert(.tools) }
        if LoginItem.isEnabled, NotificationStatus.shared.auth == .allowed { done.insert(.startup) }
        flow.satisfied(done)
    }

    private func triedIt() {
        guard flow.step == .shortcut else { return }
        WelcomeSound.success()
        withAnimation(.spring(response: 0.3, dampingFraction: 0.55)) { shortcutHits += 1 }
    }

    // MARK: - Footer

    private var footer: some View {
        // Back on the left, the choices on the right; where you are shows
        // above the title (4/7), so no dots here to crowd the button.
        HStack(spacing: 14) {
            if flow.canGoBack, flow.step != .done {
                Button(L.t("Back", "上一步")) { move(forward: false) { $0.back() } }
                    .buttonStyle(.plain)
                    .focusable(false)
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .fixedSize()
                    .handCursor()
            } else {
                StepDots(flow: flow)
            }
            Spacer(minLength: 8)
            if pending != nil, !connecting, !asking {
                Button(L.t("Later", "以后再说")) { move(forward: true) { $0.skipStep() } }
                    .buttonStyle(.plain)
                    .focusable(false)
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .fixedSize()
                    .handCursor()
                    .transition(.opacity)
            }
            BigButton(title: primaryTitle, icon: primaryIcon, busy: connecting || asking) {
                if let p = pending {
                    doPending(p)
                } else if flow.step == .done {
                    WelcomeSound.finish()
                    close(openPanel: true)
                } else {
                    move(forward: true) { $0.next() }
                }
            }
            .disabled(connecting || asking)
            .keyboardShortcut(.defaultAction)
        }
    }

    private var primaryTitle: String {
        if connecting { return L.t("Connecting…", "正在连接…") }
        switch pending {
        case .connect: return L.t("Connect", "连接并继续")
        case .notify: return L.t("Allow", "允许提醒并继续")
        case .accessibility: return L.t("Allow", "去允许")
        case nil: break
        }
        return switch flow.step {
        case .welcome: L.t("Let's Go", "开始吧")
        case .done: L.t("Open My Sessions", "打开我的会话")
        default: L.t("Continue", "继续")
        }
    }

    private var primaryIcon: String {
        switch pending {
        case .connect: "link"
        case .notify: "bell"
        case .accessibility: "arrow.up.forward.app"
        case nil: flow.step == .done ? "checkmark" : "arrow.right"
        }
    }

    private func move(forward: Bool, _ change: @escaping (inout WelcomeFlow) -> Void) {
        self.forward = forward
        if forward { WelcomeSound.step() }
        // Out quickly, swap, then rise in one by one.
        withAnimation(.easeIn(duration: reduceMotion ? 0.1 : 0.14)) { shown = false }
        DispatchQueue.main.asyncAfter(deadline: .now() + (reduceMotion ? 0.1 : 0.15)) {
            withAnimation(.spring(response: 0.42, dampingFraction: 0.86)) { change(&flow) }
            enter()
        }
    }

    private func enter() {
        withAnimation(.spring(response: 0.5, dampingFraction: 0.82)) { shown = true }
    }

    private func close(openPanel: Bool) {
        withAnimation(.easeIn(duration: 0.2)) { appeared = false }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { finish(openPanel) }
    }

    // MARK: - Right: the stage

    private var stage: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(LinearGradient(colors: stageColors, startPoint: .topLeading, endPoint: .bottomTrailing))
                .animation(.easeInOut(duration: 0.5), value: flow.step)
            WelcomeStage(step: flow.step, dark: scheme == .dark, hits: shortcutHits,
                         shortcut: shortcut, ready: plan?.isReady == true,
                         still: live ? nil : 2.0)
                .id(flow.step)
                .scaleEffect(shown ? 1 : 0.94)
                .opacity(shown ? 1 : 0)
        }
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).strokeBorder(Theme.hairline(scheme)))
    }

    private var stageColors: [Color] {
        let dark = scheme == .dark
        func c(_ r: Double, _ g: Double, _ b: Double) -> Color {
            dark ? Color(red: r * 0.32, green: g * 0.32, blue: b * 0.36) : Color(red: r, green: g, blue: b)
        }
        switch flow.step {
        case .welcome, .done: return [c(1.0, 0.86, 0.80), c(0.99, 0.94, 0.90)]
        case .tools: return [c(0.86, 0.90, 1.0), c(0.94, 0.96, 1.0)]
        case .workspace: return [c(0.99, 0.92, 0.80), c(1.0, 0.96, 0.91)]
        case .shortcut: return [c(0.90, 0.86, 1.0), c(0.96, 0.94, 1.0)]
        case .startup: return [c(1.0, 0.87, 0.87), c(1.0, 0.94, 0.93)]
        case .mo: return [c(0.82, 0.95, 0.91), c(0.92, 0.98, 0.96)]
        }
    }
}

// MARK: - Motion

private extension View {
    /// Rises in from below and the side you're going, after `order` beats.
    func rise(_ shown: Bool, order: Int, forward: Bool) -> some View {
        self
            .opacity(shown ? 1 : 0)
            .offset(x: shown ? 0 : (forward ? 18 : -18), y: shown ? 0 : 8)
            .animation(.spring(response: 0.5, dampingFraction: 0.84).delay(shown ? Double(order) * 0.06 : 0), value: shown)
    }
}

/// Soft system sounds for the welcome, like Raycast's: a pop forward, a
/// chime when something got done, a flourish at the end. Silent when
/// "Play user interface sound effects" is off.
@MainActor
enum WelcomeSound {
    private static var enabled: Bool {
        (UserDefaults(suiteName: ".GlobalPreferences")?.object(forKey: "com.apple.sound.uiaudio.enabled") as? Int) != 0
    }

    private static func play(_ name: String, volume: Float) {
        guard enabled, let s = NSSound(named: NSSound.Name(name))?.copy() as? NSSound else { return }
        s.volume = volume
        s.play()
    }

    static func step() { play("Pop", volume: 0.18) }
    static func success() { play("Glass", volume: 0.25) }
    static func finish() { play("Hero", volume: 0.22) }
}

// MARK: - Pieces

/// The main button: tall, in the brand colour, with a gentle press.
private struct BigButton: View {
    let title: String
    let icon: String
    var busy = false
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                if busy { ProgressView().controlSize(.small) }
                Text(title).font(.system(size: 14, weight: .semibold)).lineLimit(1).fixedSize()
                if !busy { Image(systemName: icon).font(.system(size: 12, weight: .bold)) }
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 18)
            .frame(height: 38)
            .background(Capsule().fill(Theme.brand.opacity(hovering ? 0.9 : 1)))
            .shadow(color: Theme.brand.opacity(0.35), radius: hovering ? 10 : 6, y: 3)
            .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .handCursor()
        .onHover { inside in withAnimation(.easeOut(duration: 0.15)) { hovering = inside } }
    }
}

/// A soft card around a choice.
private struct ChoiceCard<Content: View>: View {
    @ViewBuilder let content: () -> Content
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        content()
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Color.primary.opacity(scheme == .dark ? 0.06 : 0.035)))
    }
}

/// The workspace: a folder, or the last one used.
private struct WorkspaceChoice: View {
    @AppStorage(NewSession.workspaceKey) private var workspace = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Image(systemName: workspace.isEmpty ? "folder" : "folder.fill")
                    .font(.system(size: 18))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 26)
                Text(workspace.isEmpty ? L.t("The folder I used last", "上次用的目录") : (workspace as NSString).lastPathComponent)
                    .font(.system(size: 13, weight: .semibold))
                    .lineLimit(1)
                Spacer(minLength: 6)
                SoftButton(title: workspace.isEmpty ? L.t("Choose…", "选择…") : L.t("Change…", "换一个…"),
                           icon: Icons.chooseFolder) { choose() }
            }
            // The whole path on its own line: long ones wrap once, then
            // lose their middle, never their ends.
            Text(workspace.isEmpty ? L.t("Or pick a workspace to always start in.", "也可以选一个固定的工作区。")
                                   : Grouping.tilde(workspace, home: NSHomeDirectory()))
                .font(.system(size: 11, design: workspace.isEmpty ? .default : .monospaced))
                .foregroundStyle(Theme.caption)
                .lineLimit(2)
                .truncationMode(.middle)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.leading, 36)
        }
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.prompt = L.t("Use This Folder", "用这个目录")
        panel.message = L.t("New sessions will start here.", "以后新会话都从这里开始。")
        if panel.runModal() == .OK, let url = panel.url {
            withAnimation(.spring(response: 0.35, dampingFraction: 0.8)) { workspace = url.path }
        }
    }
}

/// Which terminal sessions open in.
private struct TerminalChoice: View {
    let prefs: TerminalPrefs

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "macwindow").font(.system(size: 16)).foregroundStyle(Theme.brand).frame(width: 26)
            Text(L.t("Open sessions in", "会话在这里打开")).font(.system(size: 13, weight: .semibold))
            Spacer(minLength: 6)
            TerminalPicker(prefs: prefs)
        }
    }
}

/// The shortcut, already set to double Control; change it right here, and
/// try it: the stage plays along.
private struct ShortcutChoice: View {
    let tried: Bool
    @Binding var shortcut: PanelShortcut
    private var trust: HotKeyCenter.Trust { HotKeyCenter.shared.trust }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HotKeyRecorder(onSave: { shortcut = $0 }, large: true)
            if shortcut.tapKey != nil, !trust.allowed {
                Label(L.t("macOS asks you to allow this once.", "macOS 需要你允许一次，点下面的按钮就好。"),
                      systemImage: "hand.raised.fill")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .transition(.opacity)
            } else if shortcut != .none {
                Label(tried ? L.t("Nice, that's it. Again?", "成功！再按一次试试？")
                            : L.t("Try it now: \(shortcut.spoken).", "现在就试试：\(shortcut.spoken)。"),
                      systemImage: tried ? "checkmark.circle.fill" : "hand.point.up.left")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(tried ? Color.green : Color.secondary)
                    .transition(.opacity)
            }
        }
        .animation(.spring(response: 0.35, dampingFraction: 0.85), value: trust.allowed)
        .animation(.spring(response: 0.35, dampingFraction: 0.85), value: shortcut)
    }
}

/// Open at login (suggested on: the one time we ask, A8) and notifications.
private struct StartupChoice: View {
    let live: Bool
    @State private var openAtLogin = true
    private var status: NotificationStatus { NotificationStatus.shared }

    var body: some View {
        VStack(spacing: 10) {
            ChoiceCard {
                HStack(spacing: 10) {
                    Image(systemName: "power").foregroundStyle(Theme.brand).frame(width: 22)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(L.t("Start with my Mac", "开机就在")).font(.system(size: 13, weight: .semibold))
                        Text(L.t("Waits quietly in the menu bar", "安安静静待在菜单栏里")).font(.system(size: 11)).foregroundStyle(Theme.caption)
                    }
                    Spacer()
                    Toggle("", isOn: $openAtLogin)
                        .toggleStyle(.switch).labelsHidden().controlSize(.small).cursor(.pointingHand)
                        .onChange(of: openAtLogin) { _, on in apply(on) }
                }
            }
            ChoiceCard {
                HStack(spacing: 10) {
                    Image(systemName: "bell.badge").foregroundStyle(Theme.brand).frame(width: 22)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(L.t("Let us tell you", "允许提醒你")).font(.system(size: 13, weight: .semibold))
                        Text(L.t("When a session needs you or stops", "会话需要你、或者断了的时候")).font(.system(size: 11)).foregroundStyle(Theme.caption)
                    }
                    Spacer()
                    switch status.auth {
                    case .allowed:
                        Image(systemName: "checkmark.circle.fill").font(.system(size: 18)).foregroundStyle(.green)
                            .transition(.scale.combined(with: .opacity))
                    case .denied:
                        SoftButton(title: L.t("Settings", "去设置"), icon: "gearshape") { NotificationStatus.openSystemSettings() }
                    default:
                        Text(L.t("Not yet", "还没允许")).font(.system(size: 12)).foregroundStyle(.tertiary)
                    }
                }
                .animation(.spring(response: 0.35, dampingFraction: 0.6), value: status.auth)
            }
        }
        .onAppear {
            guard live else { return }
            if !LoginItem.isEnabled { apply(true) }
            openAtLogin = LoginItem.isEnabled
            Task { await status.refresh() }
        }
    }

    private func apply(_ on: Bool) {
        guard live else { return }
        try? LoginItem.set(on)
        openAtLogin = LoginItem.isEnabled
    }
}

/// Mo: in or not, and whether it speaks up.
private struct MoChoice: View {
    @AppStorage(PetController.enabledKey) private var enabled = false
    @AppStorage(VoiceMode.key) private var voice = VoiceMode.off.rawValue

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                pill(L.t("Yes, invite it", "好呀，请它来"), on: enabled) { set(true) }
                pill(L.t("Not now", "先不用"), on: !enabled) { set(false) }
            }
            if enabled {
                ChoiceCard {
                    VStack(alignment: .leading, spacing: 10) {
                        Label(L.t("When something needs you, it…", "有事的时候，它会…"), systemImage: "speaker.wave.2")
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(.primary)
                        SlidingPicker(selection: $voice, options: [
                            (VoiceMode.off.rawValue, L.t("Stay quiet", "安静示意")),
                            (VoiceMode.chime.rawValue, L.t("Chime", "叮一声")),
                            (VoiceMode.speak.rawValue, L.t("Say it", "说出来")),
                        ])
                        .frame(maxWidth: .infinity)
                        .onChange(of: voice) { _, _ in changed() }
                    }
                }
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .animation(.spring(response: 0.35, dampingFraction: 0.85), value: enabled)
    }

    private func pill(_ title: String, on: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 13, weight: on ? .semibold : .regular))
                .foregroundStyle(on ? Color.white : Color.primary)
                .padding(.horizontal, 14)
                .frame(height: 32)
                .background(Capsule().fill(on ? Theme.brand : Color.primary.opacity(0.06)))
                .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .handCursor()
    }

    private func set(_ on: Bool) {
        enabled = on
        changed()
    }

    private func changed() {
        NotificationCenter.default.post(name: PetController.settingsChanged, object: nil)
    }
}

/// The dots under a step: the current one long, done ones in colour.
private struct StepDots: View {
    let flow: WelcomeFlow

    var body: some View {
        HStack(spacing: 5) {
            ForEach(WelcomeFlow.Step.allCases, id: \.self) { s in
                Capsule()
                    .fill(fill(s))
                    .frame(width: s == flow.step ? 16 : 5, height: 5)
            }
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.8), value: flow.step)
        .accessibilityLabel(L.t("Step \(flow.progress.0) of \(flow.progress.1)", "第 \(flow.progress.0) 步，共 \(flow.progress.1) 步"))
    }

    private func fill(_ s: WelcomeFlow.Step) -> Color {
        if s == flow.step { return Theme.brand }
        if flow.state(of: s) == .done || s.rawValue < flow.step.rawValue { return Theme.brand.opacity(0.35) }
        return Color.primary.opacity(0.15)
    }
}

// MARK: - The window

/// The welcome's window: shown once at first launch (fading in), or from
/// Settings › About. It fades out when you're done, then the panel opens.
@MainActor
enum WelcomeWindow {
    private static var window: NSWindow?
    /// What "Open My Sessions" opens; set by the app delegate.
    static var openPanel: () -> Void = {}
    /// While the welcome is up, the shortcut proves itself here instead of opening the panel.
    static var onShortcut: (() -> Void)?
    static var isShown: Bool { window != nil }

    static func show(model: AppModel) {
        if let window {
            bring(window)
            return
        }
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: WelcomeView.size),
                         styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: false)
        w.titlebarAppearsTransparent = true
        w.titleVisibility = .hidden
        w.isMovableByWindowBackground = true
        w.isReleasedWhenClosed = false
        w.animationBehavior = .none
        w.title = L.t("Welcome to cc-moss", "欢迎使用 cc-moss")
        w.standardWindowButton(.miniaturizeButton)?.isHidden = true
        w.standardWindowButton(.zoomButton)?.isHidden = true
        let host = NSHostingView(rootView: WelcomeView(model: model) { open in
            close()
            if open { DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { openPanel() } }
        })
        // No title-bar inset: the view fills the window to its edge.
        host.safeAreaRegions = []
        w.contentView = host
        w.center()
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: w, queue: .main) { _ in
            MainActor.assumeIsolated {
                // Closed, skipped or finished: either way, not again at launch.
                WelcomeFlow.markSeen(.standard)
                window = nil
                onShortcut = nil
            }
        }
        window = w
        bring(w)
    }

    /// Back in front, e.g. after allowing something in System Settings.
    static func bringForward() {
        if let window { bring(window) }
    }

    private static func bring(_ w: NSWindow) {
        NSApp.activate(ignoringOtherApps: true)
        w.makeKeyAndOrderFront(nil)
    }

    static func close() {
        window?.close()
    }
}

/// 中 / EN at the top of the welcome: switches the whole app (and the
/// terminal's cc-moss, which shares the setting) right away.
private struct LanguageSwitch: View {
    let client: MossClient?
    @Environment(\.colorScheme) private var scheme
    private var lang: Lang { LanguageState.shared.lang }

    var body: some View {
        HStack(spacing: 0) {
            item("中", .zh)
            item("EN", .en)
        }
        .padding(2)
        .background(Capsule().fill(Color.primary.opacity(0.06)))
        .accessibilityLabel(L.t("Language", "语言"))
    }

    private func item(_ title: String, _ target: Lang) -> some View {
        let on = lang == target
        return Button {
            guard !on else { return }
            Task { try? await LanguageState.shared.choose(target == .zh ? .zh : .en, client: client) }
        } label: {
            Text(title)
                .font(.system(size: 11, weight: on ? .semibold : .regular))
                .foregroundStyle(on ? Color.primary : Color.secondary)
                .frame(width: 30, height: 20)
                .background(Capsule().fill(on ? Theme.raised(scheme) : .clear))
                .shadow(color: .black.opacity(on ? 0.08 : 0), radius: 1, y: 0.5)
                .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .handCursor()
        .animation(.easeOut(duration: 0.15), value: on)
    }
}
