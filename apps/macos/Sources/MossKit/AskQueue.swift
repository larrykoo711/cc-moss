import Foundation

/// Everything waiting on you, as one queue you work through (user
/// 2026-10-18): questions in the order they came, then sessions waiting for
/// you to type. Collapsed it is one line and the panel works as usual;
/// expanded it shows one item at a time. Answering one moves on to the next,
/// and an empty queue folds itself away.
public struct AskQueue: Equatable, Sendable {
    public enum Item: Hashable, Sendable {
        /// A question, by its id.
        case ask(String)
        /// A session waiting for a reply, by its panel id.
        case reply(String)
    }

    public var expanded = false
    /// The item you were looking at, and where it sat: when it goes, the one
    /// that took its place is next.
    private var current: Item?
    private var position = 0

    public init() {}

    /// The items in queue order.
    public static func items(cards: [AskCard], replies: [Session]) -> [Item] {
        cards.map { .ask($0.id) } + replies.map { .reply($0.id) }
    }

    /// The item shown: the one you were on while it's there, else the one
    /// now at its place, else the last.
    public func shown(in items: [Item]) -> Item? {
        guard !items.isEmpty else { return nil }
        if let current, items.contains(current) { return current }
        return items[min(position, items.count - 1)]
    }

    /// Where the shown item is, counting from 0.
    public func index(in items: [Item]) -> Int? {
        shown(in: items).flatMap { items.firstIndex(of: $0) }
    }

    /// Go to the previous (-1) or next (+1) item; stays put at either end.
    public mutating func step(_ by: Int, in items: [Item]) {
        guard let i = index(in: items) else { return }
        go(to: max(0, min(items.count - 1, i + by)), in: items)
    }

    /// Open the queue on `item` (a notification or Mo's "Open Panel").
    public mutating func show(_ item: Item, in items: [Item]) {
        expanded = true
        if let i = items.firstIndex(of: item) { go(to: i, in: items) }
    }

    /// Nothing left: fold away, so the next arrival starts as one line again.
    public mutating func emptied() {
        expanded = false
        current = nil
        position = 0
    }

    private mutating func go(to i: Int, in items: [Item]) {
        current = items[i]
        position = i
    }
}
