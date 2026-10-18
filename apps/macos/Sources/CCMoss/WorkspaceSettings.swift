import AppKit
import MossKit
import SwiftUI

/// Where new sessions start (Claude Code works in a folder, so this matters):
/// a workspace you choose, or simply the folder you used last.
struct WorkspaceRow: View {
    var divider = false
    @AppStorage(NewSession.workspaceKey) private var workspace = ""

    var body: some View {
        CardRow(title: L.t("Start new sessions in", "新会话从这里开始"), detail: detail, divider: divider) {
            HStack(spacing: 8) {
                if !workspace.isEmpty {
                    SoftButton(title: L.t("Last Used", "用最近的"), icon: "clock.arrow.circlepath") { workspace = "" }
                        .tip(L.t("Start where you worked last", "从上次工作的目录开始"))
                }
                SoftButton(title: L.t("Choose…", "选择…"), icon: Icons.chooseFolder) { choose() }
            }
        }
    }

    private var detail: String {
        guard !workspace.isEmpty else {
            return L.t("The folder you used last. Pick one to always start there.",
                       "默认是上次用的文件夹，也可以固定一个。")
        }
        if !FileManager.default.fileExists(atPath: workspace) {
            return L.t("\(Grouping.tilde(workspace, home: NSHomeDirectory())) is gone; using the last folder for now.",
                       "\(Grouping.tilde(workspace, home: NSHomeDirectory())) 不见了，先用上次的目录。")
        }
        return Grouping.tilde(workspace, home: NSHomeDirectory())
    }

    private func choose() {
        FilePicker.choose(.folder, from: NSApp.keyWindow, startingAt: workspace.isEmpty ? nil : workspace,
                          message: L.t("New sessions will start here.", "以后新会话都从这里开始。"),
                          prompt: L.t("Use This Folder", "用这个目录")) { workspace = $0 }
    }
}
