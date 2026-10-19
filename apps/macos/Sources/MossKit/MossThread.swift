import Foundation

/// Messages said in one go, shown as one bubble (user 2026-10-19): a run of
/// plain messages from the same side, each within 10 minutes of the one
/// before it.
public struct MossGroup: Identifiable, Equatable, Sendable {
    public var messages: [MossMessage]

    /// The first message's id: stable while the group grows at its end, so
    /// scroll anchors keep working.
    public var id: String { messages[0].id }
    public var from: MossMessage.Sender { messages[0].from }
    public var first: MossMessage { messages[0] }
    public var last: MossMessage { messages[messages.count - 1] }
}

public enum MossThread {
    /// How far apart two messages can be and still read as one go.
    public static let window: TimeInterval = 10 * 60

    /// A plain message, one that can share a bubble: words from you or Moss,
    /// not a command (its card, or your "/usage" that goes with it), not a
    /// refusal, not a "got it" receipt.
    public static func mergeable(_ m: MossMessage) -> Bool {
        switch m.from {
        case .moss: return m.command == nil && !m.refused && !m.fallback
        case .you: return !MossCommand.looksLikeCommand(m.text)
        default: return false
        }
    }

    /// Nothing more joins after it: Moss's suggested replies go under the
    /// whole bubble, and a message of yours that didn't get through keeps its
    /// "Resend" right beside it.
    static func closes(_ m: MossMessage) -> Bool {
        (m.from == .moss && (!m.buttons.isEmpty || !m.sessions.isEmpty)) || (m.from == .you && m.delivery == .failed)
    }

    public static func groups(_ messages: [MossMessage]) -> [MossGroup] {
        var out: [MossGroup] = []
        for m in messages {
            if var current = out.last, joins(m, after: current.last) {
                current.messages.append(m)
                out[out.count - 1] = current
            } else {
                out.append(MossGroup(messages: [m]))
            }
        }
        return out
    }

    static func joins(_ m: MossMessage, after previous: MossMessage) -> Bool {
        guard m.from == previous.from, mergeable(m), mergeable(previous), !closes(previous),
              let at = m.at, let before = previous.at else { return false }
        let gap = at.timeIntervalSince(before)
        return gap >= 0 && gap <= window
    }
}
