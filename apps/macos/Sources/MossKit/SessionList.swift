import Foundation

/// The Sessions page as one flat list: section headings and rows, each with
/// an id that stays the same wherever the row goes (user 2026-10-19). One
/// list, not a list per section, so a row moving into Pinned (or out) is the
/// same row moving, never a second copy left behind with yesterday's look.
public enum SessionListItem: Identifiable, Equatable, Sendable {
    case pinnedHeader
    case projectHeader(Project)
    case olderHeader
    /// A session, and whether it sits in Pinned (and so is pinned).
    case row(Session, pinned: Bool)
    /// While searching: a ranked result (closed: from the closed ones).
    case hit(SearchHit, closed: Bool)
    case closedHeader
    /// Nothing found, live or closed: offer to ask Moss.
    case noMatch(String)

    public var id: String {
        switch self {
        case .pinnedHeader: "header:pinned"
        case let .projectHeader(p): "header:project:\(p.path)"
        case .olderHeader: "header:older"
        // The session's own id: also what scrolling to a session uses.
        case let .row(s, _): s.id
        case let .hit(h, closed): closed ? "closed:\(h.session.id)" : h.session.id
        case .closedHeader: "header:closed"
        case .noMatch: "no-match"
        }
    }
}

public enum SessionList {
    /// While a query is typed: one ranked list (no folders, no Pinned), then
    /// the closed ones that match; nothing at all → the "ask Moss" row.
    public static func searchItems(query: String, live: [SearchHit], closed: [SearchHit]) -> [SessionListItem] {
        if live.isEmpty && closed.isEmpty { return [.noMatch(query.trimmingCharacters(in: .whitespaces))] }
        var out = live.map { SessionListItem.hit($0, closed: false) }
        if !closed.isEmpty {
            out.append(.closedHeader)
            out += closed.map { .hit($0, closed: true) }
        }
        return out
    }

    public static func items(pinned: [Session], projects: [Project], older: [Session], olderExpanded: Bool) -> [SessionListItem] {
        var out: [SessionListItem] = []
        var seen = Set<String>()
        func add(_ s: Session, pinned: Bool) {
            // A session shows once: where it's pinned, else in its folder.
            guard seen.insert(s.id).inserted else { return }
            out.append(.row(s, pinned: pinned))
        }
        if !pinned.isEmpty {
            out.append(.pinnedHeader)
            pinned.forEach { add($0, pinned: true) }
        }
        for p in projects {
            let rows = p.sessions.filter { !seen.contains($0.id) }
            guard !rows.isEmpty else { continue }
            out.append(.projectHeader(p))
            rows.forEach { add($0, pinned: false) }
        }
        if !older.isEmpty {
            out.append(.olderHeader)
            if olderExpanded { older.forEach { add($0, pinned: false) } }
        }
        return out
    }
}
