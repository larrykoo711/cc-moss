import Foundation

/// One entry of `cc-moss events --json` (schema 1). The app uses the kind,
/// the name, when it happened and, for `needs_you`, why.
public struct SessionEvent: Decodable, Equatable, Sendable {
    public var seq: Int
    public var slotId: String
    public var name: String?
    public var kind: String
    /// When the hook recorded it; nil if the CLI left it out or it did not parse.
    public var at: Date?
    /// `needs_you`'s `detail.reason`: `permission`, `question`, `input`.
    public var reason: String?

    public init(seq: Int, slotId: String, name: String? = nil, kind: String, at: Date? = nil, reason: String? = nil) {
        self.seq = seq
        self.slotId = slotId
        self.name = name
        self.kind = kind
        self.at = at
        self.reason = reason
    }

    enum CodingKeys: String, CodingKey { case seq, slotId, name, kind, at, detail }
    private struct Detail: Decodable { var reason: String? }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        seq = try c.decode(Int.self, forKey: .seq)
        slotId = try c.decode(String.self, forKey: .slotId)
        name = try c.decodeIfPresent(String.self, forKey: .name)
        kind = try c.decode(String.self, forKey: .kind)
        // Both are extras: a shape we don't expect must not cost the event.
        let rawAt = (try? c.decodeIfPresent(String.self, forKey: .at)) ?? nil
        at = rawAt.flatMap(parseTimestamp)
        let detail = (try? c.decodeIfPresent(Detail.self, forKey: .detail)) ?? nil
        reason = detail?.reason
    }
}

/// Where the app is in the event stream. The first page only places the
/// cursor (no replay of old news on launch); later pages yield what is new.
public struct EventCursor: Equatable, Sendable {
    public private(set) var lastSeq: Int?

    public init(lastSeq: Int? = nil) {
        self.lastSeq = lastSeq
    }

    /// What to pass as `--since`: nil until the cursor is placed.
    public var since: Int? { lastSeq }

    /// Take a page fetched with `since`; returns the events to act on.
    public mutating func advance(_ page: EventPage) -> [SessionEvent] {
        guard let last = lastSeq else {
            lastSeq = page.lastSeq
            return []
        }
        let fresh = page.events.filter { $0.seq > last }.sorted { $0.seq < $1.seq }
        // A fresh registry starts counting again: follow it rather than wait forever.
        lastSeq = max(page.lastSeq, fresh.last?.seq ?? page.lastSeq)
        return fresh
    }
}

/// A page of events and where to continue from.
public struct EventPage: Equatable, Sendable {
    public var events: [SessionEvent]
    public var lastSeq: Int

    public static func decode(_ data: Data) throws -> EventPage {
        struct Head: Decodable { var schema: Int }
        struct Wire: Decodable {
            var events: [SessionEvent]?
            var lastSeq: Int
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let head: Head
        do {
            head = try decoder.decode(Head.self, from: data)
        } catch {
            throw SnapshotError.invalid("\(error)")
        }
        if head.schema > 1 { throw SnapshotError.newerSchema(head.schema) }
        do {
            let wire = try decoder.decode(Wire.self, from: data)
            return EventPage(events: wire.events ?? [], lastSeq: wire.lastSeq)
        } catch {
            throw SnapshotError.invalid("\(error)")
        }
    }
}

extension MossClient {
    /// Events after `since`. With nil, only the latest one: enough to learn
    /// where the stream is without replaying old news.
    public func events(since: Int?) async throws -> EventPage {
        let args = since.map { ["events", "--json", "--since", "\($0)"] } ?? ["events", "--json", "--limit", "1"]
        let out = try await plain(args)
        return try EventPage.decode(Data(out.utf8))
    }
}
