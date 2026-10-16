import MossKit
import SwiftUI

/// Settings › General › Desktop Pet: Mo on or off, and how big.
struct PetSettings: View {
    @AppStorage(PetController.enabledKey) private var enabled = false
    @AppStorage(PetController.sizeKey) private var size = Double(PetController.defaultSize)

    var body: some View {
        SettingsCard(title: L.t("Desktop Pet", "桌面宠物")) {
            CardRow(
                title: L.t("Mo", "小莫"),
                detail: enabled
                    ? L.t("Lives in a corner of your desktop and shows how your sessions are doing. Click it to open the panel.",
                          "住在桌面一角，用表情告诉你会话怎么样了。点它打开面板。")
                    : L.t("A small companion for your desktop. It stays quiet until something needs you.",
                          "桌面上的小伙伴。平时安安静静，有事才找你。"),
                divider: true
            ) {
                Toggle("", isOn: $enabled)
                    .toggleStyle(.switch)
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: enabled) { _, _ in changed() }
            }
            CardRow(title: L.t("Size", "大小"), divider: false) {
                SlidingPicker(selection: $size, options: [
                    (48, L.t("Small", "小")),
                    (64, L.t("Medium", "中")),
                    (88, L.t("Large", "大")),
                ])
                .onChange(of: size) { _, _ in changed() }
                .disabled(!enabled)
                .opacity(enabled ? 1 : 0.45)
            }
        }
    }

    private func changed() {
        NotificationCenter.default.post(name: PetController.settingsChanged, object: nil)
    }
}
