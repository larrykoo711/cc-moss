import Foundation

/// One entry of `cc-moss events --json` (schema 1). The app uses the kind
/// and the name; `detail` is left out.
public struct SessionEvent: Decodable, Equatable, Sendable {
    public var seq: Int
    public var slotId: String
    public var name: String?
    public var kind: String
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
