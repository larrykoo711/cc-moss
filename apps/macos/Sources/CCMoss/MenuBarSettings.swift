import MossKit
import SwiftUI

/// Settings › Menu Bar: what the status item shows (A18).
struct MenuBarSettings: View {
    let model: AppModel
    @AppStorage(AppModel.showCountKey) private var showCount = false
    @AppStorage(MenuBarIcon.motionKey) private var motion = true

    var body: some View {
        SettingsCard(title: L.t("Icon", "图标")) {
            CardRow(
                title: L.t("Show in menu bar", "菜单栏里显示"),
                detail: L.t("Counts what's waiting on you first, then the rest.",
                            "数字先数在等你的，再数断了和在忙的。"),
                icon: RowIcon("menubar.rectangle", .indigo)
            ) {
                SlidingPicker(selection: $showCount, options: [
                    (false, L.t("Icon only", "只显示图标")),
                    (true, L.t("Icon and count", "图标加数字")),
                ])
                .onChange(of: showCount) { _, _ in model.syncIcon() }
            }
            CardRow(
                title: L.t("Animation", "动态效果"),
                detail: motion
                    ? L.t("The icon moves gently while sessions work. Costs a little battery.",
                          "会话在忙时，图标会轻轻动，稍微费一点电。")
                    : L.t("The icon stays still and costs nothing.",
                          "图标保持不动，不占资源。"),
                divider: false,
                icon: RowIcon("wand.and.stars", .indigo)
            ) {
                Toggle("", isOn: $motion)
                    .toggleStyle(.switch)
                    .cursor(.pointingHand)
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: motion) { _, _ in model.syncIcon() }
            }
        }
    }
}
