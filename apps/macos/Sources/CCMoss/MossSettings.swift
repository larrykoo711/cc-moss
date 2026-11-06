import AppKit
import MossKit
import SwiftUI

/// Settings › Moss: Moss itself (on or off, how long it has run), then
/// the cc-moss daemon it lives in, with Start when that isn't running.
struct MossSettings: View {
    let model: AppModel
    /// Screenshots pass a fixed check and nothing is asked of cc-moss.
    var demo: DaemonCheck?
    var mossDemo: MossCheck?
    @State private var check: DaemonCheck = .checking
    @State private var starting = false
    @State private var problem: String?

    private var shown: DaemonCheck { demo ?? check }

    private var detail: String {
        if let problem { return problem }
        if case .failed(let why) = shown { return why }
        return L.t("It starts when you log in, and comes back on its own if it stops. It starts Moss and keeps it running.",
                   "登录时自动启动，意外退出也会自己回来。Moss 由它启动和照看。")
    }

    var body: some View {
        MossCard(model: model, demo: mossDemo)
        MossCheckupCard(model: model, demo: demo != nil || mossDemo != nil)
        MossSearchCard(model: model, demo: demo != nil || mossDemo != nil)
        SettingsCard(title: L.t("Background service", "后台服务")) {
            CardRow(title: shown.line(),
                    detail: detail,
                    divider: false) {
                control
            }
        }
        Text(L.t("You rarely need to touch it. To stop it, and Moss with it, run cc-moss daemon stop in Terminal; it stays off until you start it again.",
                 "一般不用管它。真要停掉（Moss 也会一起停），在终端运行 cc-moss daemon stop；之后会一直停着，直到你再启动。"))
            .font(.system(size: 11))
            .foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 12)
            .task(id: model.cliPath) {
                guard demo == nil else { return }
                while !Task.isCancelled {
                    await refresh()
                    try? await Task.sleep(for: .seconds(5))
                }
            }
    }

    @ViewBuilder private var control: some View {
        if starting {
            ProgressView().controlSize(.small)
        } else if let status = shown.status, status.running {
            HStack(spacing: 4) {
                Image(systemName: "checkmark.circle.fill").font(.system(size: 12, weight: .semibold)).foregroundStyle(.green)
                Text(L.t("Running", "运行中")).font(.system(size: 12)).foregroundStyle(.secondary)
            }
            .fixedSize()
            .accessibilityElement(children: .combine)
        } else if shown.status != nil {
            SoftButton(title: L.t("Start", "启动"), icon: "play.fill", prominent: true) { start() }
                .disabled(model.client == nil || demo != nil)
        }
    }

    private func refresh() async {
        guard let client = model.client else {
            check = .failed(L.t("The app can't find the cc-moss command.", "App 找不到 cc-moss 命令。"))
            return
        }
        do {
            let status = try await client.daemonStatus()
            check = .known(status)
            if status.running { problem = nil }
        } catch {
            check = .failed(AppModel.describe(error))
        }
    }

    private func start() {
        guard let client = model.client, !starting else { return }
        starting = true
        Task {
            do {
                try await client.startDaemon()
                problem = nil
            } catch {
                problem = L.t("Didn't start: \(error)", "没能启动：\(error)")
            }
            await refresh()
            starting = false
        }
    }
}

/// The Moss card: its status line and the switch. Off means cc-moss ends it
/// and leaves it off; nothing else in the app can end it.
private struct MossCard: View {
    let model: AppModel
    var demo: MossCheck?
    @State private var check: MossCheck = .checking
    @State private var on = true
    @State private var busy = false
    @State private var problem: String?

    private var shown: MossCheck { demo ?? check }

    var body: some View {
        SettingsCard(title: "Moss") {
            CardRow(title: shown.line(),
                    detail: problem ?? shown.status?.problem() ?? L.t("An always-on helper that looks after your sessions and answers your questions. It uses your own Claude Code account, and costs nothing while it waits.",
                                           "一直在线的小助手，帮你照看会话、回答问题。它用你自己的 Claude Code 账号，没事做时不花额度。")) {
                if busy {
                    ProgressView().controlSize(.small)
                } else {
                    Toggle("", isOn: $on)
                        .toggleStyle(.switch)
                        .cursor(.pointingHand)
                        .labelsHidden()
                        .controlSize(.small)
                        .disabled(model.client == nil || shown.status == nil)
                        .onChange(of: on) { _, value in
                            if value != shown.status?.enabled { set(value) }
                        }
                }
            }
            CardRow(title: L.t("Talk to Moss", "和 Moss 聊聊"),
                    detail: L.t("Ask it something, or try a quick action.", "问它点什么，或者试试快捷动作。")) {
                SoftButton(title: L.t("Open", "打开"), icon: "bubble.left.and.text.bubble.right") {
                    if demo == nil { AppDelegate.show(.moss) }
                }
            }
            CardRow(title: L.t("Moss's folder", "Moss 的文件夹"),
                    detail: L.t("Its memory, skills and settings live here.", "它的记忆、技能和设置都在这里。"),
                    divider: false) {
                SoftButton(title: L.t("Show in Finder", "在 Finder 中显示"), icon: "folder") {
                    if demo == nil, let home = shown.status?.home {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: home)])
                    }
                }
                .disabled(shown.status?.home == nil)
            }
        }
        Text(L.t("To give Moss a rest, turn it off here. It stays off until you turn it back on.",
                 "想让 Moss 休息就在这里关掉，它会一直关着，直到你再打开。"))
            .font(.system(size: 11))
            .foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 12)
            .onAppear { if let s = demo?.status { on = s.enabled } }
            .task(id: model.cliPath) {
                guard demo == nil else { return }
                while !Task.isCancelled {
                    await refresh()
                    try? await Task.sleep(for: .seconds(5))
                }
            }
    }

    private func refresh() async {
        guard let client = model.client else {
            check = .failed(L.t("The app can't find the cc-moss command.", "App 找不到 cc-moss 命令。"))
            return
        }
        do {
            let status = try await client.mossStatus()
            check = .known(status)
            if !busy { on = status.enabled }
        } catch {
            check = .failed(AppModel.describe(error))
        }
    }

    private func set(_ value: Bool) {
        guard let client = model.client, demo == nil, !busy else { return }
        busy = true
        Task {
            do {
                try await client.setMoss(value)
                problem = nil
            } catch {
                problem = value
                    ? L.t("Didn't turn on: \(error)", "没能打开：\(error)")
                    : L.t("Didn't turn off: \(error)", "没能关掉：\(error)")
            }
            await refresh()
            busy = false
        }
    }
}

/// Settings › Moss › Conversations (F44): whether Moss may search what was
/// said in your sessions. Off by default: what it finds goes to the model.
/// Off and greyed when cc-moss doesn't know the setting (an older one).
private struct MossSearchCard: View {
    let model: AppModel
    var demo = false
    @State private var on = false
    @State private var known = false
    /// Writes the switch to the config, last value wins (nil until loaded).
    @State private var writer: SwitchWriter?
    @State private var problem: String?

    var body: some View {
        SettingsCard(title: L.t("Conversations", "对话内容")) {
            CardRow(title: L.t("Let Moss search conversations", "允许 Moss 搜索对话内容"),
                    detail: L.t("So it can find a session by what you said in it.",
                                "让它能按你说过的话帮你找会话。"),
                    divider: false) {
                Toggle("", isOn: $on)
                    .toggleStyle(.switch)
                    .cursor(.pointingHand)
                    .labelsHidden()
                    .controlSize(.small)
                    .disabled(!known && !demo)
                    .onChange(of: on) { _, value in save(value) }
            }
        }
        Text(problem ?? L.t("What it finds, a few words from each match, goes to the model with your question. Searching from the panel stays on this Mac and needs no switch.",
                            "它找到的内容（每个结果的一小段原话）会随你的问题发给模型。在面板里自己搜索只在本机进行，不需要打开这个开关。"))
            .font(.system(size: 11))
            .foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 12)
            .task(id: model.cliPath) { await load() }
    }

    private func load() async {
        guard !demo, let client = model.client else { return }
        guard let value = try? await client.config(MossSearch.key) else {
            // This cc-moss doesn't know the setting (an older one).
            known = false
            writer = nil
            return
        }
        let holds = MossSearch.isOn(value)
        if let writer {
            writer.loaded(holds)
        } else {
            writer = SwitchWriter(
                holds: holds,
                write: { try await client.setConfig(MossSearch.key, $0 ? "true" : "false") },
                read: { MossSearch.isOn(try await client.config(MossSearch.key)) },
                describe: { AppModel.describe($0) })
        }
        on = holds
        known = true
    }

    private func save(_ value: Bool) {
        guard known, !demo, let writer else { return }
        Task {
            switch await writer.set(value) {
            case .unchanged, .superseded:
                return
            case .done(let holds, let failed):
                if let holds { on = holds }
                problem = failed.map { L.t("Didn't save: \($0)", "没能保存：\($0)") }
            }
        }
    }
}

/// Settings › Moss › Checkup (T2.4): the daily checkup and its brief, kept
/// in cc-moss's config. Both on by default; the brief needs the checkup.
private struct MossCheckupCard: View {
    let model: AppModel
    /// Screenshots: both on, nothing asked of cc-moss.
    var demo = false
    @State private var checkup = true
    @State private var brief = true
    @State private var loaded = false
    @State private var problem: String?

    var body: some View {
        SettingsCard(title: L.t("Checkup", "体检")) {
            CardRow(title: L.t("Daily checkup", "每天体检")) {
                toggle($checkup, key: MossDaily.checkupKey)
            }
            CardRow(title: L.t("Daily brief", "每天发体检简报"), divider: false) {
                toggle($brief, key: MossDaily.briefKey)
                    .disabled(!checkup)
            }
        }
        Text(problem ?? L.t("Moss checks up once a day when you first open the panel. With the brief off, it speaks up only when you need to decide.",
                            "每天第一次打开面板时，Moss 自动体检一次；简报关掉后它只在需要你拿主意时开口。"))
            .font(.system(size: 11))
            .foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 12)
            .task(id: model.cliPath) { await load() }
    }

    private func toggle(_ value: Binding<Bool>, key: String) -> some View {
        Toggle("", isOn: value)
            .toggleStyle(.switch)
            .cursor(.pointingHand)
            .labelsHidden()
            .controlSize(.small)
            .onChange(of: value.wrappedValue) { _, on in save(key, on) }
    }

    private func load() async {
        guard !demo, let client = model.client else { return }
        // A cc-moss without these keys answers with an error: that's "on".
        checkup = MossDaily.isOn(try? await client.config(MossDaily.checkupKey))
        brief = MossDaily.isOn(try? await client.config(MossDaily.briefKey))
        loaded = true
    }

    private func save(_ key: String, _ on: Bool) {
        guard loaded, !demo, let client = model.client else { return }
        Task {
            do {
                try await client.setConfig(key, MossDaily.value(on))
                problem = nil
            } catch {
                problem = L.t("Didn't save: \(AppModel.describe(error))", "没能保存：\(AppModel.describe(error))")
            }
        }
    }
}
