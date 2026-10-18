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
        return L.t("It starts when you log in, and comes back on its own if it stops. Moss runs inside it.",
                   "登录时自动启动，意外退出也会自己回来。Moss 就跑在它里面。")
    }

    var body: some View {
        MossCard(model: model, demo: mossDemo)
        SettingsCard(title: L.t("In the Background", "后台")) {
            CardRow(title: shown.line(),
                    detail: detail,
                    divider: false) {
                control
            }
        }
        Text(L.t("To stop it, run cc-moss daemon stop in Terminal; it stays off until you start it again.",
                 "要停掉它，在终端运行 cc-moss daemon stop；之后会一直停着，直到你再启动。"))
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
                    detail: problem ?? L.t("A Claude Code session cc-moss keeps running for you. It uses your Claude Code sign-in, and while it waits it uses none of your plan.",
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
