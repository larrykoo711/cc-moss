import AppKit
import MossKit
import SwiftUI

/// A saved preference is only a candidate; the backend approves it again at launch.
struct WorkspaceRow: View {
    @Bindable var model: AppModel
    var divider = false
    @AppStorage(NewSession.workspaceKey) private var workspace = ""
    @State private var request = UUID()
    @State private var checking = false
    @State private var error: String?

    var body: some View {
        CardRow(title: L.t("Start new sessions in", "新会话从这里开始"), detail: detail, divider: divider) {
            HStack(spacing: 8) {
                if !workspace.isEmpty {
                    SoftButton(title: L.t("Last Used", "用最近的"), icon: "clock.arrow.circlepath") {
                        request = UUID(); workspace = ""; checking = false; error = nil
                    }
                    .tip(L.t("Use the most recent approved folder", "使用最近通过检查的工作目录"))
                }
                SoftButton(title: L.t("Choose…", "选择…"), icon: Icons.chooseFolder) { choose() }
                    .disabled(checking)
            }
        }
        .onDisappear { request = UUID() }
    }

    private var detail: String {
        if let error { return error }
        if checking { return L.t("Checking working folder…", "正在检查工作目录…") }
        guard !workspace.isEmpty else {
            return L.t("The most recent approved folder, or choose a project folder.", "使用最近通过检查的工作目录；没有合适目录时需要手动选择。")
        }
        return Grouping.tilde(workspace, home: NSHomeDirectory())
    }

    private func choose() {
        guard let client = model.client, model.cliCapabilities.checksWorkingDirectory else {
            error = WorkingDirectoryPolicy.updateRequired
            return
        }
        let token = UUID()
        request = token
        let capabilities = model.cliCapabilities
        FilePicker.choose(.folder, from: NSApp.keyWindow, startingAt: workspace.isEmpty ? nil : workspace,
                          message: L.t("New sessions will start here.", "以后新会话都从这里开始。"),
                          prompt: L.t("Use This Folder", "用这个目录")) { path in
            guard request == token else { return }
            checking = true
            error = nil
            Task { @MainActor in
                do {
                    let checks = try await client.checkDirectories([path], capabilities: capabilities)
                    guard request == token else { return }
                    checking = false
                    guard let check = checks.first, check.approvedForNewSession, let canonical = check.canonical else {
                        error = checks.first?.explanation ?? WorkingDirectoryReason.unavailable.explanation
                        return
                    }
                    workspace = canonical
                } catch {
                    guard request == token else { return }
                    checking = false
                    self.error = AppModel.describe(error)
                }
            }
        }
    }
}
