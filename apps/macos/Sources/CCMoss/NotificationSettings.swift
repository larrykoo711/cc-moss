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
            CardRow(title: L.t("Who Tells You", "谁来告诉你"), detail: modeDetail) {
                SlidingPicker(selection: $mode, options: [
                    (AlertMode.auto.rawValue, L.t("Automatic", "自动")),
                    (AlertMode.notificationsOnly.rawValue, L.t("Notifications", "通知中心")),
                    (AlertMode.petOnly.rawValue, L.t("Mo", "小莫")),
                ])
            }
            AuthRow(status: status)
        }
        SettingsCard(title: L.t("What to Tell You", "提醒哪些事")) {
            KindRow(kind: .needsYou,
                    title: L.t("Needs You", "需要你确认"),
                    detail: L.t("A permission prompt or a question. It reaches cc-moss a few seconds after it appears.",
                                "权限请求或提问。弹出后过几秒才会传到 cc-moss。"))
            KindRow(kind: .interrupted,
                    title: L.t("Session Stopped", "会话断了"),
                    detail: L.t("The process went away without anyone ending it. The alert offers to bring it back.",
                                "进程没打招呼就没了。提醒里可以直接找回。"))
            KindRow(kind: .finished,
                    title: L.t("Turn Finished", "这一轮做完了"),
                    detail: L.t("Every turn ends with one, so this gets chatty. Off unless you like company.",
                                "每一轮结束都会提醒，有点话多，默认关着。"))
            KindRow(kind: .restored,
                    title: L.t("Session Back", "找回完成"),
                    detail: L.t("A stopped session is running again.", "断了的会话又跑起来了。"),
                    divider: false)
        }
        Text(L.t("The same alert from the same session within 30 seconds counts once. While the panel is open, nothing pops up: it's all in front of you already.",
                 "同一个会话 30 秒内的同类提醒只算一次。面板开着的时候不另外提醒，反正都在眼前了。"))
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

/// Whether macOS lets cc-moss post, and the one thing to do about it.
private struct AuthRow: View {
    let status: NotificationStatus

    var body: some View {
        CardRow(title: L.t("Notification Center", "通知中心"), detail: detail, divider: false) {
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
        case .allowed: L.t("Allowed", "已允许")
        case .denied: L.t("Turned off in System Settings", "在系统设置里被关掉了")
        case .unavailable: L.t("Not available here", "这里用不了")
        case .notDetermined, .unknown: L.t("Not asked yet", "还没问过你")
        }
    }

    private var detail: String { title + L.t(". ", "。") + explanation }

    private var explanation: String {
        switch status.auth {
        case .allowed:
            L.t("Banners can open a session or bring it back.",
                "横幅上能直接打开或找回。")
        case .denied:
            L.t("Turn them on in Notifications › cc-moss.",
                "到「通知 › cc-moss」里打开即可。")
        case .unavailable:
            L.t("Needs the app in an Applications folder.",
                "App 要放在「应用程序」文件夹里才能发通知。")
        case .notDetermined, .unknown:
            L.t("macOS will ask the first time there's something to tell you. Or let it ask now.",
                "第一次有事要告诉你时，macOS 会问你一次。也可以现在就问。")
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
