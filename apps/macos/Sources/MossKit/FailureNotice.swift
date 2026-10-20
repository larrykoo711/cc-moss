import Foundation

/// Something the app asked cc-moss to do that didn't work (F25): what it
/// was, which session, and why. One shape for every action, so a failure is
/// never silent: the panel shows it in plain words, offers what to try, and
/// can hand the whole thing to Moss.
public struct FailureNotice: Equatable, Identifiable, Sendable {
    /// What was being done.
    public enum Action: String, Equatable, Sendable {
        case restore, restart, archive, unarchive, endAndArchive, adopt, send, answer, newSession, close, note, rename

        /// "找回" / "bring back": for the title and the message to Moss.
        var verb: String {
            switch self {
            case .restore: L.t("bring back", "找回")
            case .restart: L.t("restart", "重开")
            case .archive: L.t("archive", "归档")
            case .unarchive: L.t("take out of Archived", "拿回")
            case .endAndArchive: L.t("end and archive", "结束并归档")
            case .adopt: L.t("move into tmux", "移进 tmux")
            case .send: L.t("send a message to", "发消息给")
            case .answer: L.t("answer", "回答")
            case .newSession: L.t("start", "新建")
            case .close: L.t("stop reminding about", "收起")
            case .note: L.t("save the note on", "保存便条到")
            case .rename: L.t("rename", "重命名")
            }
        }

        /// The action's name in the message to Moss: 「找回」 / "Bring back".
        var name: String {
            switch self {
            case .restore: L.t("Bring back", "找回")
            case .restart: L.t("Restart", "重开")
            case .archive: L.t("Archive", "归档")
            case .unarchive: L.t("Bring back from Archived", "拿回来")
            case .endAndArchive: L.t("End and archive", "结束并归档")
            case .adopt: L.t("Move into tmux", "移进 tmux")
            case .send: L.t("Send a message", "发消息")
            case .answer: L.t("Answer", "回答")
            case .newSession: L.t("New session", "新建会话")
            case .close: L.t("Stop reminding", "不再提醒")
            case .note: L.t("Note", "写便条")
            case .rename: L.t("Rename", "重命名")
            }
        }
    }

    /// The session it was about.
    public struct Subject: Equatable, Sendable {
        public var name: String
        /// The start of its conversation id (8 characters).
        public var id: String?
        public var folder: String?

        public init(name: String, id: String? = nil, folder: String? = nil) {
            self.name = name
            self.id = id.map { String($0.prefix(8)) }
            self.folder = folder
        }

        public init(_ s: Session) {
            self.init(name: s.title, id: s.sessionId, folder: s.cwd)
        }
    }

    public var id: String
    public var action: Action
    public var subject: Subject?
    /// cc-moss's reason code (`missing_dir`, `busy`, …), when it gave one.
    public var reason: String?
    /// cc-moss's own words, or the error's last meaningful line.
    public var message: String?
    /// For `ambiguous`: the sessions the name could mean.
    public var candidates: [String]

    public init(id: String = UUID().uuidString, action: Action, subject: Subject?, reason: String? = nil,
                message: String? = nil, candidates: [String] = []) {
        self.id = id
        self.action = action
        self.subject = subject
        self.reason = reason?.isEmpty == true ? nil : reason
        let m = message?.trimmingCharacters(in: .whitespacesAndNewlines)
        self.message = m?.isEmpty == true ? nil : m
        self.candidates = candidates
    }

    /// From an error thrown by a `MossClient` call.
    public init(action: Action, subject: Subject?, error: Error) {
        let (reason, message): (String?, String?) = switch error {
        case let MossClient.Failure.command(m): (nil, m)
        case MossClient.Failure.timedOut: ("timeout", nil)
        case let MossClient.Failure.newerSchema(n): ("newer_schema", "schema \(n)")
        case let MossClient.Failure.unreadable(m): ("unreadable", m)
        default: (nil, "\(error)")
        }
        self.init(action: action, subject: subject, reason: reason, message: message)
    }

    /// From one failed line of `restore --json`.
    public init(restore r: RestoreResult, subject: Subject?) {
        self.init(action: .restore, subject: subject ?? Subject(name: r.name, id: nil),
                  reason: r.reasonCode, message: r.message, candidates: r.candidates)
    }

    /// From one refused line of `archive|unarchive|restart --json`.
    public init(action: Action, batch r: BatchResult, subject: Subject?) {
        self.init(action: action, subject: subject ?? Subject(name: r.name, id: r.sessionId),
                  reason: r.reason ?? r.outcome, message: nil)
    }

    // MARK: Words

    private var who: String { subject?.name ?? L.t("the session", "这个会话") }

    /// "Couldn't bring back “X”" / 「没能找回「X」」.
    public var title: String {
        if action == .newSession, subject == nil { return L.t("Couldn't start a new session", "没能新建会话") }
        return L.t("Couldn't \(action.verb) “\(who)”", "没能\(action.verb)「\(who)」")
    }

    /// One line: why, and what you can try.
    public var why: String {
        switch reason {
        case "empty": return L.t("It never said a word, so there's nothing to bring back. Stop reminding about it.",
                                 "它还没说过话，没什么可找回的，可以不再提醒它。")
        case "missing_dir": return L.t("Its folder is gone. Put the folder back, or stop reminding about it.",
                                       "它的文件夹不在了。把文件夹放回去，或者不再提醒它。")
        case "exited_at_once": return L.t("Claude Code quit right after starting. Run cc-moss doctor in Terminal, or ask Moss.",
                                          "Claude Code 一启动就退出了。在终端里运行 cc-moss doctor，或者交给 Moss。")
        case "tmux_failed": return L.t("tmux couldn't start it. Check that tmux works, or ask Moss.",
                                       "tmux 没能把它开起来。看看 tmux 是否正常，或者交给 Moss。")
        case "already_running": return L.t("It's already running somewhere. Look for it in the list.",
                                           "它已经在别处运行了，在列表里找找它。")
        case "ambiguous":
            let list = candidates.prefix(4).joined(separator: L.t(", ", "、"))
            return candidates.isEmpty
                ? L.t("More than one session has this name. Ask Moss to sort them out.", "有好几个会话叫这个名字，交给 Moss 理一理。")
                : L.t("More than one session matches: \(list). Ask Moss to sort them out.", "有好几个会话对得上：\(list)。交给 Moss 理一理。")
        case "busy": return L.t("It's working right now. Try again when this turn is done.", "它正在干活，等这轮做完再试。")
        case "running": return L.t("It's still running. End it first.", "它还在运行，先结束它。")
        case "not-running": return L.t("It isn't running any more.", "它已经不在运行了。")
        case "not-in-tmux": return L.t("It isn't in tmux. Move it into tmux first.", "它不在 tmux 里，先把它移进去。")
        case "in-tmux": return L.t("It's already in tmux.", "它已经在 tmux 里了。")
        case "moss": return L.t("cc-moss looks after Moss itself.", "Moss 由 cc-moss 自己照看。")
        case "timeout": return L.t("cc-moss took too long to answer. Try again, or ask Moss.", "cc-moss 太久没回话。再试一次，或者交给 Moss。")
        case "newer_schema": return L.t("This app is older than your cc-moss. Update both.", "这个 App 比 cc-moss 旧，两个一起升级。")
        default:
            if let message { return L.t("cc-moss said: \(message)", "cc-moss 说：\(message)") }
            return L.t("cc-moss didn't say why. Ask Moss to look into it.", "cc-moss 没说原因，交给 Moss 看看。")
        }
    }

    /// The reason as Moss reads it: the code, else nothing.
    private var reasonForMoss: String? { reason }

    /// What "Ask Moss to fix it" sends.
    public var mossText: String {
        var parts: [String] = []
        if let reasonForMoss { parts.append(reasonForMoss) }
        var detail = message ?? ""
        if !candidates.isEmpty {
            let list = candidates.joined(separator: L.t(", ", "、"))
            detail += (detail.isEmpty ? "" : L.t("; ", "；")) + L.t("candidates: \(list)", "候选：\(list)")
        }
        if !detail.isEmpty { parts.append(detail) }
        let cause = parts.isEmpty ? L.t("no reason given", "没有给出原因") : parts.joined(separator: L.t(": ", "："))
        guard let subject else {
            return L.t("“\(action.name)” failed: \(cause). Please help me fix it.",
                       "「\(action.name)」失败了：\(cause)。帮我看看怎么解决。")
        }
        let where_ = [subject.id, subject.folder].compactMap { $0 }.filter { !$0.isEmpty }
        let tail = where_.isEmpty ? "" : L.t(" (\(where_.joined(separator: ", ")))", "（\(where_.joined(separator: "，"))）")
        return L.t("“\(action.name)” failed for session \(subject.name)\(tail): \(cause). Please help me fix it.",
                   "「\(action.name)」会话 \(subject.name)\(tail)失败了：\(cause)。帮我看看怎么解决。")
    }

    /// "Copy details": everything, one fact per line.
    public var details: String {
        var lines = [title, why, L.t("Action: \(action.rawValue)", "动作：\(action.rawValue)")]
        if let subject {
            lines.append(L.t("Session: \(subject.name)", "会话：\(subject.name)"))
            if let id = subject.id { lines.append(L.t("Id: \(id)", "编号：\(id)")) }
            if let folder = subject.folder { lines.append(L.t("Folder: \(folder)", "文件夹：\(folder)")) }
        }
        if let reason { lines.append(L.t("Reason: \(reason)", "原因：\(reason)")) }
        if let message { lines.append(L.t("cc-moss said: \(message)", "cc-moss 说：\(message)")) }
        if !candidates.isEmpty { lines.append(L.t("Candidates: ", "候选：") + candidates.joined(separator: ", ")) }
        return lines.joined(separator: "\n")
    }
}

/// The notices waiting to be seen: the newest shows, the rest count as "+N".
/// Each stays until you close it.
public struct FailureNotices: Equatable, Sendable {
    public private(set) var list: [FailureNotice] = []

    public init() {}

    public var current: FailureNotice? { list.last }
    /// How many more are behind the one shown.
    public var more: Int { max(0, list.count - 1) }

    public mutating func add(_ n: FailureNotice) {
        list.append(n)
    }

    /// Close the one shown; the one before it shows next.
    public mutating func dismiss() {
        if !list.isEmpty { list.removeLast() }
    }

    public mutating func dismiss(_ id: String) {
        list.removeAll { $0.id == id }
    }
}
