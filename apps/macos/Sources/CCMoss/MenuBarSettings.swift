import MossKit
import SwiftUI

/// Settings › General › Menu Bar: what the status item shows (A18).
struct MenuBarSettings: View {
    let model: AppModel
    @AppStorage(AppModel.showCountKey) private var showCount = false
    @AppStorage(MenuBarIcon.motionKey) private var motion = true

    var body: some View {
        SettingsCard(title: L.t("Menu Bar", "菜单栏")) {
            CardRow(
                title: L.t("Show", "显示"),
                detail: L.t("Counts what needs you first, then what broke or is busy.",
                            "先数需要你的，再数断了的、在忙的。")
            ) {
                SlidingPicker(selection: $showCount, options: [
                    (false, L.t("Icon", "仅图标")),
                    (true, L.t("Icon and Number", "图标 + 数字")),
                ])
                .onChange(of: showCount) { _, _ in model.syncIcon() }
            }
            CardRow(
                title: L.t("Motion", "动态效果"),
                detail: motion
                    ? L.t("The icon moves while a session works or needs you. Uses a little CPU (about 1–2%).",
                          "会话在忙或需要你时，图标会动。会用一点 CPU（约 1–2%）。")
                    : L.t("The icon stays still and only changes when the state does. Costs nothing.",
                          "图标保持静止，只在状态变化时换样子，不占资源。"),
                divider: false
            ) {
                Toggle("", isOn: $motion)
                    .toggleStyle(.switch)
                    .handCursor()
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: motion) { _, _ in model.syncIcon() }
            }
        }
    }
}
