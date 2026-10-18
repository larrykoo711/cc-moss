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
        let primary: RowAction = if s.state == .interrupted {
            hopeless ? .stopReminding : .bringBack
        } else if !s.inTmux {
            .copyPath
        } else {
            .open
        }
        // Renaming is a message too (`/rename`), so both need a running session.
        let talk: [RowAction] = Message.allowed(s) ? [.rename, .message] : []
        let copies: [RowAction] = if s.state == .interrupted {
            [.copyRestoreCommand, .copyPath]
        } else if s.inTmux {
            [.copyCommand, .copyPath]
        } else {
            [.copyPath]
        }
        let close: [RowAction] = Close.allowed(s) && primary != .stopReminding ? [.close] : []

        let inline = talk + [primary]
        let all = [[primary] + talk.reversed(), [.note], copies.filter { $0 != primary }, close]
            .filter { !$0.isEmpty }
        self.primary = primary
        self.inline = inline
        self.all = all
        more = all.map { $0.filter { !inline.contains($0) } }.filter { !$0.isEmpty }
    }
}
