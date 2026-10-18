import Foundation

/// What a session is doing, as the picker shows it.
public enum SessionState: Equatable, Sendable {
    /// Claude is working on a turn.
    case working
    /// The turn is over; it is your move.
    case waiting
    /// The process is gone without anyone ending it: it can be brought back.
    case interrupted
    case closed

    /// For the summary strip, where three must fit side by side.
    public var shortLabel: String {
        switch self {
        case .working: L.t("Working", "在忙")
        case .waiting: L.t("Waiting", "在等你")
        case .interrupted: L.t("Interrupted", "已中断")
        case .closed: L.t("Closed", "已结束")
        }
    }

    public var label: String {
        switch self {
        case .working: L.t("Working", "在忙")
        case .waiting: L.t("Waiting for you", "在等你")
        case .interrupted: L.t("Interrupted", "已中断")
        case .closed: L.t("Closed", "已结束")
        }
    }
}

extension Session {
    public var state: SessionState {
        switch status {
        case "running": busy ? .working : .waiting
        case "interrupted": .interrupted
        default: .closed
        }
    }

    /// The session's name; unnamed ones fall back like the picker does: the
    /// tmux session name without its host prefix, else the start of the id.
    public var title: String {
        if let name, !name.isEmpty { return name }
        if isMoss { return "Moss" }
        if let tmuxName = tmux?.sessionName, !tmuxName.isEmpty {
            if let cut = tmuxName.firstIndex(of: "_") {
                let rest = tmuxName[tmuxName.index(after: cut)...]
                if !rest.isEmpty { return String(rest) }
            }
            return tmuxName
        }
        return String(sessionId.prefix(8))
    }

    /// The second line: your note if there is one, else the last thing you said.
    public var subtitle: String? {
        let note = [notes.note, notes.state, notes.goal]
            .compactMap { $0?.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first { !$0.isEmpty }
        if let note { return note }
        let prompt = lastPrompt?.trimmingCharacters(in: .whitespacesAndNewlines)
        return prompt?.isEmpty == false ? prompt : nil
    }

    /// True when the note, not the last prompt, fills the second line.
    public var subtitleIsNote: Bool {
        [notes.note, notes.state, notes.goal].contains { $0?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false }
    }

    /// Only sessions living in tmux can be opened from the menubar.
    public var inTmux: Bool { tmux?.sessionId != nil || tmux?.sessionName != nil }
}
