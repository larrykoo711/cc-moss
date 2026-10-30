import Foundation

/// Something you can do to one session from its row.
public enum RowAction: Hashable, Sendable {
    case open
    case bringBack
    /// Nothing left to bring back: dismiss it right away.
    case stopReminding
    case message
    case rename
    case note
    case copyCommand
    case copyRestoreCommand
    case copyPath
    /// "Stop reminding…", asked first.
    case close
    /// Outside tmux: move it in (`cc-moss adopt`), asked first.
    case adopt
    /// Not running: put it away in Archived (`cc-moss archive`).
    case archive
    /// Running but idle: end it and archive it (`cc-moss archive --end`), asked first.
    case endAndArchive
    /// Archived: back into the list (`cc-moss unarchive`).
    case unarchive
    /// Archived: ask Moss to delete it for good (Moss asks you first).
    case cleanUp
    /// Ordinary restart in place, without applying the provider model.
    case restart
    case providerPlan
}

/// Which actions a session's row shows and which wait behind its "⋯": the
/// few you use most sit on the row, the main one last (next to "⋯"); the
/// rest are grouped in the menu.
public struct RowActions: Equatable, Sendable {
    /// Buttons on the row, not counting "⋯".
    public static let inlineLimit = 3

    public let primary: RowAction
    /// Left to right; the main action is the last.
    public let inline: [RowAction]
    /// Behind "⋯": everything not on the row, in groups.
    public let more: [[RowAction]]
    /// Right-click and ⌘K: every action, the main one first.
    public let all: [[RowAction]]

    public init(_ s: Session, hopeless: Bool) {
        // Archived: bring it back, or have Moss clean it up.
        if s.isArchived && !s.isMoss {
            primary = .unarchive
            inline = [.cleanUp, .unarchive]
            all = [[.unarchive, .cleanUp], [.copyPath]]
            more = [[.copyPath]]
            return
        }
        // cc-moss brings Moss back itself: nothing to bring back or dismiss.
        let primary: RowAction = if s.isMoss {
            s.inTmux ? .open : .copyPath
        } else if s.state == .interrupted {
            hopeless ? .stopReminding : .bringBack
        } else if !s.canOpen {
            .copyPath
        } else {
            .open
        }
        // Renaming is a message too (`/rename`), so both need a running
        // session. Moss's name lives in its own files, not in /rename.
        let talk: [RowAction] = !Message.allowed(s) ? [] : s.isMoss ? [.message] : [.rename, .message]
        let copies: [RowAction] = if s.state == .interrupted {
            [.copyRestoreCommand, .copyPath]
        } else if s.canOpen {
            [.copyCommand, .copyPath]
        } else {
            [.copyPath]
        }
        // Moss is never dismissed: only switching it off in Settings stops it.
        let close: [RowAction] = Close.allowed(s) && primary != .stopReminding && !s.isMoss ? [.close] : []

        // Outside tmux and running: it can move in (busy ones wait, but the
        // button stays to say so).
        let adopt: [RowAction] = Adopt.availability(s) == .none ? [] : [.adopt]
        // Ordinary restart belongs in the menu, independent of provider flags.
        let restart: [RowAction] = Restart.allowed(s) ? [.restart] : []
        let provider: [RowAction] = ProviderRecovery.state(s) != nil ? [.providerPlan] : []
        let archive: [RowAction] = Archive.allowed(s) ? [.archive] : Archive.endAllowed(s) ? [.endAndArchive] : []

        // Left to right: move into tmux, send a
        // message, open, rename. Too many: rename waits in "⋯".
        var inline = adopt + talk.filter { $0 == .message } + [primary] + talk.filter { $0 == .rename }
        if inline.count > Self.inlineLimit { inline.removeAll { $0 == .rename } }
        let all = [[primary] + provider + restart + adopt + talk.reversed(), [.note], copies.filter { $0 != primary }, close + archive]
            .filter { !$0.isEmpty }
        self.primary = primary
        self.inline = inline
        self.all = all
        // "Move into tmux" stays in "⋯" too: it's easy to miss as an icon.
        more = all.map { $0.filter { !inline.contains($0) || $0 == .adopt } }.filter { !$0.isEmpty }
    }
}
