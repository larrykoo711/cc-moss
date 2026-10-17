import MossKit
import SwiftUI

/// Settings › Notifications: where alerts go (A23), which events count
/// (A9), and a word when macOS won't let cc-moss use Notification Center
/// (the full picture is on the Permissions page).
struct NotificationSettings: View {
    var status: NotificationStatus = .shared
    @AppStorage(AlertMode.key) private var mode = AlertMode.auto.rawValue
    @AppStorage(PetController.enabledKey) private var petEnabled = false

    var body: some View {
        SettingsCard(title: L.t("Delivery", "提醒方式")) {
            CardRow(title: L.t("Who lets you know", "谁来提醒你"), detail: modeDetail, divider: status.auth != .allowed) {
                SlidingPicker(selection: $mode, options: [
                    (AlertMode.auto.rawValue, L.t("Automatic", "自动")),
                    (AlertMode.notificationsOnly.rawValue, L.t("Notifications", "系统通知")),
                    (AlertMode.petOnly.rawValue, L.t("Mo", "小莫")),
                ])
            }
            if status.auth != .allowed {
                PermissionRow(kind: .notifications, divider: false)
            }
        }
        .task { await status.refresh() }
        SettingsCard(title: L.t("What to Tell You", "提醒哪些事")) {
            KindRow(kind: .needsYou,
                    title: L.t("Waiting for you", "在等你"),
                    detail: L.t("When Claude needs a yes or an answer.", "Claude 要你同意或回答问题时。"))
            KindRow(kind: .interrupted,
                    title: L.t("Session stopped", "会话断了"),
                    detail: L.t("When one stops unexpectedly. Nothing's lost.", "意外停下时告诉你，进度都还在。"))
            KindRow(kind: .finished,
                    title: L.t("Turn finished", "这一轮做完了"),
                    detail: L.t("Comes after every reply, so it's off by default.", "每轮都会响，有点吵，所以默认关着。"))
            KindRow(kind: .restored,
                    title: L.t("Back again", "找回来了"),
                    detail: L.t("When a stopped session is running again.", "断了的会话重新跑起来时。"),
                    divider: false)
        }
        Text(L.t("One session's same alert counts once in 30 seconds, and nothing pops up while the panel is open.",
                 "同一个会话 30 秒内只提醒一次；面板开着时不再另外提醒。"))
            .font(.system(size: 11))
            .foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 12)
    }

    private var modeDetail: String {
        switch AlertMode(rawValue: mode) ?? .auto {
        case .auto:
            petEnabled
                ? L.t("Mo while it's on screen, otherwise Notification Center.",
                      "小莫在桌面时由它提醒，否则走通知中心。")
                : L.t("Mo is off, so Notification Center tells you.",
                      "小莫没开，由通知中心提醒。")
        case .notificationsOnly:
            L.t("Always Notification Center; Mo just shows it on its face.",
                "一律走通知中心，小莫只用表情示意。")
        case .petOnly:
            petEnabled
                ? L.t("Only Mo, even when it's tucked away.",
                      "只让小莫说，收在边上也一样。")
                : L.t("Mo is off; Notification Center fills in.",
                      "小莫没开，先由通知中心代班。")
        }
    }
}

/// One kind of alert, on or off.
private struct KindRow: View {
    let kind: AlertKind
    let title: String
    let detail: String
    var divider = true
    @AppStorage private var on: Bool

    init(kind: AlertKind, title: String, detail: String, divider: Bool = true) {
        self.kind = kind
        self.title = title
        self.detail = detail
        self.divider = divider
        _on = AppStorage(wrappedValue: kind.defaultOn, AlertToggles.key(kind))
    }

    var body: some View {
        CardRow(title: title, detail: detail, divider: divider) {
            Toggle("", isOn: $on)
                .toggleStyle(.switch)
                .cursor(.pointingHand)
                .labelsHidden()
                .controlSize(.small)
                .accessibilityLabel(title)
        }
    }
}
