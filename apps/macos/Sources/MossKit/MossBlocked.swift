import Foundation

/// What Moss's Claude Code is stuck on before it can be talked to (F59), as
/// `moss status --json` says (`blocked`). cc-moss never answers it: the user
/// does, in Moss's own window.
public struct MossBlocked: Equatable, Sendable {
    public enum Kind: String, Equatable, Sendable {
        case bypassWarning = "bypass_warning"
        case login
        case unknownDialog = "unknown_dialog"
    }

    public var kind: Kind
    public var since: Date?

    public init(kind: Kind, since: Date?) {
        self.kind = kind
        self.since = since
    }
}

/// The note at the top of the Moss page while Moss is stuck.
public struct MossBlockedNotice: Equatable, Sendable {
    public var title: String
    public var detail: String
    public var button: String

    /// What the button runs in the user's terminal: Moss's own tmux session
    /// (`cc-moss moss attach`), never a session that happens to be called moss.
    public static func command(cli: String) -> String {
        LaunchPlan.cliCommand(cli: cli, ["moss", "attach"])
    }
}

extension MossStatus {
    /// The note for the top of the page, while Moss is stuck.
    public func blockedNotice(lang: Lang = L.lang) -> MossBlockedNotice? {
        guard activity == .blocked else { return nil }
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        let after = t(" Your messages go in after that.", "你的消息随后就会送进去。")
        let (title, detail): (String, String) = switch blocked?.kind ?? .unknownDialog {
        case .bypassWarning:
            (t("Moss stopped on Claude Code's warning about skipping permissions",
               "Moss 停在了 Claude Code 跳过权限确认的警告上"),
             t("Open its window and pick Yes, I accept.", "打开它的窗口，选 Yes, I accept。") + after)
        case .login:
            (t("Moss's Claude Code wants you to sign in", "Moss 的 Claude Code 要你先登录"),
             t("Open its window and sign in there.", "打开它的窗口，在那里登录。") + after)
        case .unknownDialog:
            (t("Moss stopped on a question in its window", "Moss 停在它窗口里的一个问题上"),
             t("Open its window to see what it's asking.", "打开它的窗口，看看它在问什么。") + after)
        }
        return MossBlockedNotice(title: title, detail: detail, button: t("Open Moss's window", "打开 Moss 的窗口"))
    }

    /// Under a message of yours still in Moss's inbox.
    public func queuedLabel(lang: Lang = L.lang) -> String {
        let t = { (en: String, zh: String) in lang == .zh ? zh : en }
        return switch activity {
        case .blocked: t("Waiting for Moss to get past a question", "等 Moss 处理一个确认框")
        // Queued with Moss free means it's being handed over right now.
        case .idle: t("Sending…", "发送中…")
        default: t("In line", "排队中")
        }
    }
}
