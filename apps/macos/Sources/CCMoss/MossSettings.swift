import MossKit
import SwiftUI

/// Settings › Moss: whether the cc-moss daemon (the background process
/// Moss will live in) is running, and Start when it isn't. Read only
/// otherwise: launchd keeps it going, `cc-moss daemon stop` stops it.
struct MossSettings: View {
    let model: AppModel
    /// Screenshots pass a fixed status and nothing is asked of cc-moss.
    var demo: DaemonStatus?
    @State private var status: DaemonStatus?
    @State private var starting = false
    @State private var problem: String?

    private var shown: DaemonStatus? { demo ?? status }

    var body: some View {
        SettingsCard(title: L.t("In the Background", "后台")) {
            CardRow(title: shown?.line() ?? L.t("Daemon · checking…", "守护进程 · 正在查看…"),
                    detail: problem ?? L.t("It starts when you log in, and comes back on its own if it stops. Moss will run inside it.",
                                           "登录时自动启动，意外退出也会自己回来。以后 Moss 就跑在它里面。"),
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
        } else if let shown, shown.running {
            HStack(spacing: 4) {
                Image(systemName: "checkmark.circle.fill").font(.system(size: 12, weight: .semibold)).foregroundStyle(.green)
                Text(L.t("Running", "运行中")).font(.system(size: 12)).foregroundStyle(.secondary)
            }
            .fixedSize()
            .accessibilityElement(children: .combine)
        } else if shown != nil {
            SoftButton(title: L.t("Start", "启动"), icon: "play.fill", prominent: true) { start() }
                .disabled(model.client == nil || demo != nil)
        }
    }

    private func refresh() async {
        guard let client = model.client else { return }
        do {
            status = try await client.daemonStatus()
            if status?.running == true { problem = nil }
        } catch {
            status = .stopped
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
