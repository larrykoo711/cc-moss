import Foundation

/// Sessions by what was said in them (F44): `cc-moss find --json`, read from
/// the local index the daemon keeps. Done on this Mac only; no model sees it.
public struct SaidResult: Equatable, Sendable {
    public struct Match: Equatable, Sendable {
        /// The session's id (as in `ls --json`).
        public var id: String
        /// A short piece of what was said, redacted by cc-moss.
        public var snippet: String
        public var at: Date?
    }

    public var matches: [Match]
    /// The search ran out of time: there may be more.
    public var partial: Bool
    /// The index is still being built.
    public var indexing: Bool

    private struct Wire: Decodable {
        struct M: Decodable {
            var id: String
            var snippet: String
            var at: String?
        }
        var schema: Int
        var matches: [M]
        var partial: Bool?
        var indexing: Bool?
    }

    public static func decode(_ data: Data) throws -> SaidResult {
        let wire = try JSONDecoder().decode(Wire.self, from: data)
        guard wire.schema <= 1 else { throw MossClient.Failure.newerSchema(wire.schema) }
        return SaidResult(
            matches: wire.matches.map { Match(id: $0.id, snippet: $0.snippet, at: $0.at.flatMap(parseTimestamp)) },
            partial: wire.partial ?? false,
            indexing: wire.indexing ?? false)
    }
}

public enum SaidSearch {
    /// The shortest query worth asking for: one character finds too much.
    public static let minimum = 2

    /// Whether to look in what was said for `query`.
    public static func asks(_ query: String) -> Bool {
        query.trimmingCharacters(in: .whitespaces).count >= minimum
    }

    /// The matches as results, in cc-moss's order: sessions found in `pool`
    /// only, Moss and the ones already listed (`shown`, by id) left out.
    public static func hits(_ result: SaidResult, pool: [Session], shown: Set<String>) -> [SearchHit] {
        var seen = shown
        return result.matches.compactMap { m in
            guard let s = pool.first(where: { $0.id == m.id }), !s.isMoss, seen.insert(s.id).inserted else {
                return nil
            }
            return SearchHit(session: s, rank: SearchRank.said, nameRanges: [], reason: .said(m.snippet))
        }
    }
}

extension MossClient {
    /// `cc-moss find --json`; nil when this cc-moss can't (an older one) or didn't answer.
    public func find(_ query: String, limit: Int = 10, timeout: Double = 3) async -> SaidResult? {
        guard let out = try? await plainData(["find", "--json", "--limit", String(limit), "--", query],
                                             timeout: timeout) else { return nil }
        return try? SaidResult.decode(out)
    }
}

/// `[moss] search_history`: Moss may search what was said (F44). Off unless
/// the config says exactly true: what Moss finds goes to the model.
public enum MossSearch {
    public static let key = "moss.search_history"

    public static func isOn(_ value: String?) -> Bool {
        value?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == "true"
    }
}
