import MossKit
import SwiftUI

/// Settings › General › Desktop Pet: Mo on or off, how big, and how it
/// gets your attention (DESIGN-pet §6.5).
struct PetSettings: View {
    @AppStorage(PetController.enabledKey) private var enabled = false
    @AppStorage(PetController.sizeKey) private var size = Double(PetController.defaultSize)
    @AppStorage(PetInteraction.key) private var interaction = PetInteraction.everyday.rawValue

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
                    .handCursor()
                    .labelsHidden()
                    .controlSize(.small)
                    .onChange(of: enabled) { _, _ in changed() }
            }
            CardRow(title: L.t("Size", "大小"), divider: true) {
                SlidingPicker(selection: $size, options: [
                    (48, L.t("Small", "小")),
                    (64, L.t("Medium", "中")),
                    (88, L.t("Large", "大")),
                ])
                .onChange(of: size) { _, _ in changed() }
                .disabled(!enabled)
                .opacity(enabled ? 1 : 0.45)
            }
            CardRow(title: L.t("Interaction", "互动方式"), detail: detail, divider: false) {
                SlidingPicker(selection: $interaction, options: [
                    (PetInteraction.quiet.rawValue, L.t("Quiet", "安静")),
                    (PetInteraction.everyday.rawValue, L.t("Everyday", "日常")),
                    (PetInteraction.active.rawValue, L.t("Active", "积极")),
                ])
                .onChange(of: interaction) { _, _ in changed() }
                .disabled(!enabled)
                .opacity(enabled ? 1 : 0.45)
            }
        }
    }

    /// What the chosen mode does when something needs you.
    private var detail: String {
        switch PetInteraction(rawValue: interaction) ?? .everyday {
        case .quiet:
            L.t("Mo only makes a face. Click it to see what's up.",
                "小莫只用表情和动作示意，点它才说是什么事。")
        case .everyday:
            L.t("Mo makes a face and murmurs a word or two. Hover over it to read more.",
                "小莫用表情示意，再嘀咕一两个字。鼠标移过去就展开说。")
        case .active:
            L.t("Mo pops up the message right away, buttons and all.",
                "小莫直接弹出消息框，按钮一并给你。")
        }
    }

    private func changed() {
        NotificationCenter.default.post(name: PetController.settingsChanged, object: nil)
    }
}
