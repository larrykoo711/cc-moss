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
        SettingsCard(title: L.t("In the Background", "后台")) {
            CardRow(title: shown.line(),
                    detail: detail,
                    divider: false) {
                control
            }
        }
        Text(L.t("To stop it, and Moss with it, run cc-moss daemon stop in Terminal; it stays off until you start it again.",
                 "要停掉它（Moss 也会一起停下），在终端运行 cc-moss daemon stop；之后会一直停着，直到你再启动。"))
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
                    detail: problem ?? shown.status?.problem() ?? L.t("A Claude Code session cc-moss keeps running for you. It uses your Claude Code sign-in, and while it waits it uses none of your plan.",
                                           "cc-moss 替你一直开着的一个 Claude Code 会话。它用你的 Claude Code 登录；闲着等的时候，不耗你的额度。"),
                    divider: false) {
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
        }
        Text(L.t("Moss can't be ended from the session list. Turn it off here; it stays off until you turn it back on.",
                 "会话列表里结束不了 Moss。在这里关掉；之后会一直关着，直到你再打开。"))
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
