import MossKit
import SwiftUI

/// Settings › Moss: whether the cc-moss daemon (the background process
/// Moss will live in) is running, and Start when it isn't. Read only
/// otherwise: launchd keeps it going, `cc-moss daemon stop` stops it.
struct MossSettings: View {
    let model: AppModel
    /// Screenshots pass a fixed check and nothing is asked of cc-moss.
    var demo: DaemonCheck?
    @State private var check: DaemonCheck = .checking
    @State private var starting = false
    @State private var problem: String?

    private var shown: DaemonCheck { demo ?? check }

    private var detail: String {
        if let problem { return problem }
        if case .failed(let why) = shown { return why }
        return L.t("It starts when you log in, and comes back on its own if it stops. Moss will run inside it.",
                   "登录时自动启动，意外退出也会自己回来。以后 Moss 就跑在它里面。")
    }

    var body: some View {
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
