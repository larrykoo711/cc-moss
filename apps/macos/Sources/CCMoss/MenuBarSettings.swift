import MossKit
import SwiftUI

/// Settings › General › Menu Bar: what the status item shows (A18).
struct MenuBarSettings: View {
    let model: AppModel
    @AppStorage(AppModel.showCountKey) private var showCount = false

    var body: some View {
        SettingsCard(title: L.t("Menu Bar", "菜单栏")) {
            CardRow(
                title: L.t("Show", "显示"),
                detail: L.t("Counts what needs you first, then what broke or is busy.",
                            "先数需要你的，再数断了的、在忙的。"),
                divider: false
            ) {
                Picker("", selection: $showCount) {
                    Text(L.t("Icon", "仅图标")).tag(false)
                    Text(L.t("Icon and Number", "图标 + 数字")).tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                .onChange(of: showCount) { _, _ in model.syncIcon() }
            }
        }
    }
}
