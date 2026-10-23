import Foundation

public enum RestoreControlState: String, CaseIterable, Sendable {
    case idle, hover, pressed, focused, disabled
}

/// One interaction policy shared by native rendering and off-screen action checks.
public struct RestoreControlFeedback: Equatable, Sendable {
    public let state: RestoreControlState
    public let showsFocus: Bool
    public var isEnabled: Bool { state != .disabled }
    public var emphasized: Bool { isEnabled && state != .idle }
    public var opacity: Double { isEnabled ? 1 : 0.45 }

    public init(isEnabled: Bool, hovered: Bool = false, pressed: Bool = false, focused: Bool = false,
                preview: RestoreControlState? = nil) {
        if !isEnabled || preview == .disabled {
            state = .disabled
            showsFocus = false
        } else {
            state = preview ?? (pressed ? .pressed : focused ? .focused : hovered ? .hover : .idle)
            showsFocus = preview.map { $0 == .focused } ?? focused
        }
    }

    @discardableResult public func activate(_ action: () -> Void) -> Bool {
        guard isEnabled else { return false }
        action()
        return true
    }
}

public enum RestoreSettingsCopy {
    public static var locationLabel: String? { L.t("Settings read from", "设置读取位置") }
    public static var changeFolder: String { L.t("Change Settings Folder", "更换设置文件夹") }
    public static var chooseFolder: String { L.t("Choose Settings Folder", "选择设置文件夹") }
    public static var folderHelp: String {
        L.t("Change where settings are read for this restore. The session's working directory stays the same.",
            "仅更改本次设置读取位置，不会改变会话工作目录。")
    }
    public static func unknown(advanced: Bool) -> String {
        advanced
            ? L.t("This older session didn't record its settings source. Choose where to read settings for this restore.", "旧会话未记录设置来源，仅更改本次设置读取位置。")
            : L.t("This older session didn't record its settings source. This restore can use your current defaults.", "旧会话未记录设置来源，将使用当前默认设置。")
    }
}
