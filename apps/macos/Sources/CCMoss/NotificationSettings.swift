import MossKit
import SwiftUI

/// Settings › Notifications: where alerts go (A23), which events count
/// (A9), and whether macOS lets cc-moss use Notification Center.
struct NotificationSettings: View {
    var status: NotificationStatus = .shared
    @AppStorage(AlertMode.key) private var mode = AlertMode.auto.rawValue
    @AppStorage(PetController.enabledKey) private var petEnabled = false

    var body: some View {
        SettingsCard(title: L.t("Delivery", "提醒方式")) {
            CardRow(title: L.t("Who lets you know", "谁来提醒你"), detail: modeDetail, icon: RowIcon("bell.fill", .red)) {
                SlidingPicker(selection: $mode, options: [
                    (AlertMode.auto.rawValue, L.t("Automatic", "自动")),
                    (AlertMode.notificationsOnly.rawValue, L.t("Notifications", "系统通知")),
                    (AlertMode.petOnly.rawValue, L.t("Mo", "小莫")),
                ])
            }
            AuthRow(status: status)
        }
        SettingsCard(title: L.t("What to Tell You", "提醒哪些事")) {
            KindRow(kind: .needsYou,
                    title: L.t("Waiting for you", "在等你"),
                    detail: L.t("When Claude needs a yes or an answer.", "Claude 要你同意或回答问题时。"),
                    icon: RowIcon("hand.wave.fill", .orange))
            KindRow(kind: .interrupted,
                    title: L.t("Session stopped", "会话断了"),
                    detail: L.t("When one stops unexpectedly. Nothing's lost.", "意外停下时告诉你，进度都还在。"),
                    icon: RowIcon("bolt.slash.fill", .red))
            KindRow(kind: .finished,
                    title: L.t("Turn finished", "这一轮做完了"),
                    detail: L.t("Comes after every reply, so it's off by default.", "每轮都会响，有点吵，所以默认关着。"),
                    icon: RowIcon("checkmark.circle.fill", .green))
            KindRow(kind: .restored,
                    title: L.t("Back again", "找回来了"),
                    detail: L.t("When a stopped session is running again.", "断了的会话重新跑起来时。"),
                    icon: RowIcon("arrow.uturn.backward.circle.fill", .teal),
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
    var icon: RowIcon?
    @AppStorage private var on: Bool

    init(kind: AlertKind, title: String, detail: String, icon: RowIcon? = nil, divider: Bool = true) {
        self.kind = kind
        self.title = title
        self.detail = detail
        self.icon = icon
        self.divider = divider
        _on = AppStorage(wrappedValue: kind.defaultOn, AlertToggles.key(kind))
    }

    var body: some View {
        CardRow(title: title, detail: detail, divider: divider, icon: icon) {
            Toggle("", isOn: $on)
                .toggleStyle(.switch)
                .cursor(.pointingHand)
                .labelsHidden()
                .controlSize(.small)
                .accessibilityLabel(title)
        }
    }
}

/// Whether macOS lets cc-moss post, and the one thing to do about it.
private struct AuthRow: View {
    let status: NotificationStatus

    var body: some View {
        CardRow(title: L.t("Notifications", "系统通知"), detail: detail, divider: false, icon: RowIcon("bell.badge.fill", .red)) {
            HStack(spacing: 8) {
                Image(systemName: symbol)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(tint)
                    .accessibilityLabel(title)
                    .tip(title)
                control
            }
        }
        .task { await status.refresh() }
    }

    private var title: String {
        switch status.auth {
        case .allowed: L.t("On", "已打开")
        case .denied: L.t("Turned off in System Settings", "在系统设置里关掉了")
        case .unavailable: L.t("Not available here", "这里用不了")
        case .notDetermined, .unknown: L.t("Not asked yet", "还没问过你")
        }
    }

    private var detail: String { title + L.t(". ", "。") + explanation }

    private var explanation: String {
        switch status.auth {
        case .allowed:
            L.t("Click one to open a session or bring it back.",
                "点一下通知就能打开或找回会话。")
        case .denied:
            L.t("Turn them back on there.",
                "去那里打开就行。")
        case .unavailable:
            L.t("Move cc-moss into Applications to use them.",
                "把 cc-moss 放进「应用程序」文件夹才能用。")
        case .notDetermined, .unknown:
            L.t("macOS asks the first time there's news.",
                "第一次有事时 macOS 会问一次。")
        }
    }

    private var symbol: String {
        switch status.auth {
        case .allowed: "checkmark.circle.fill"
        case .denied, .unavailable: "exclamationmark.triangle.fill"
        case .notDetermined, .unknown: "questionmark.circle"
        }
    }

    private var tint: Color {
        switch status.auth {
        case .allowed: .green
        case .denied, .unavailable: Theme.amber
        case .notDetermined, .unknown: .secondary
        }
    }

    @ViewBuilder private var control: some View {
        switch status.auth {
        case .denied, .allowed:
            SoftButton(title: L.t("Open System Settings", "打开系统设置"), icon: Icons.settings) {
                NotificationStatus.openSystemSettings()
            }
            .handCursor()
            .tip(L.t("System Settings › Notifications", "系统设置 › 通知"))
        case .notDetermined, .unknown:
            SoftButton(title: L.t("Allow Notifications", "允许通知"), icon: "bell.badge", prominent: true) {
                Task { await status.request() }
            }
            .handCursor()
            .tip(L.t("macOS asks you once", "macOS 会问你一次"))
        case .unavailable:
            EmptyView()
        }
    }
}
