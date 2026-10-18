import Foundation

/// A question a session is waiting on, as `cc-moss ask list --json` reports
/// it (schema 1, T1.3). The app only reads and answers through the CLI; it
/// never touches the files behind it.
public struct Ask: Equatable, Identifiable, Sendable {
    public enum Kind: String, Sendable {
        /// Claude's multiple-choice question (AskUserQuestion).
        case question
        /// A `--safe` session asking to run a tool.
        case permission
    }

    public struct Option: Equatable, Sendable {
        /// Shown and answered exactly as the session wrote it.
        public var label: String
        public var description: String?

        public init(label: String, description: String? = nil) {
            self.label = label
            self.description = description
        }
    }

    public struct Question: Equatable, Sendable {
        /// The full question; it is also the key the answer goes under.
        public var text: String
        /// A word or two above the question, when the session gave one.
        public var header: String?
        public var options: [Option]
        public var multiSelect: Bool

        public init(text: String, header: String? = nil, options: [Option], multiSelect: Bool = false) {
            self.text = text
            self.header = header
            self.options = options
            self.multiSelect = multiSelect
        }
    }

    public struct Tool: Equatable, Sendable {
        public var name: String
        /// What it would run, already shortened and with secrets masked by cc-moss.
        public var preview: String

        public init(name: String, preview: String) {
            self.name = name
            self.preview = preview
        }
    }

    public var id: String
    public var kind: Kind
    public var sessionId: String
    public var sessionName: String?
    public var cwd: String
    public var createdAt: Date
    public var expiresAt: Date
    /// For `question`: at least one.
    public var questions: [Question]
    /// For `permission`.
    public var tool: Tool?

    public init(id: String, kind: Kind, sessionId: String, sessionName: String? = nil, cwd: String = "",
                createdAt: Date, expiresAt: Date, questions: [Question] = [], tool: Tool? = nil) {
        self.id = id
        self.kind = kind
        self.sessionId = sessionId
        self.sessionName = sessionName
        self.cwd = cwd
        self.createdAt = createdAt
        self.expiresAt = expiresAt
        self.questions = questions
        self.tool = tool
    }
}

public enum AskListError: Error, Equatable {
    case newerSchema(Int)
    case invalid(String)
}

/// `ask list --json`: `{"schema":1,"asks":[…]}`.
public enum AskList {
    public static let supportedSchema = 1

    /// The questions in the order the CLI lists them (oldest first). One that
    /// can't be read (a field missing, a kind this app doesn't know) is left
    /// out rather than spoiling the rest: better no card than a wrong one.
    public static func decode(_ data: Data) throws -> [Ask] {
        guard let top = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AskListError.invalid("not a JSON object")
        }
        guard let schema = top["schema"] as? Int else { throw AskListError.invalid("no schema") }
        if schema > supportedSchema { throw AskListError.newerSchema(schema) }
        let list = top["asks"] as? [Any] ?? []
        return list.compactMap { ($0 as? [String: Any]).flatMap(ask) }
    }

    static func ask(_ o: [String: Any]) -> Ask? {
        guard let id = o["id"] as? String, !id.isEmpty,
              let kind = (o["kind"] as? String).flatMap(Ask.Kind.init(rawValue:)),
              let sessionId = o["session_id"] as? String,
              let created = (o["created_at"] as? String).flatMap(parseTimestamp),
              let expires = (o["expires_at"] as? String).flatMap(parseTimestamp)
        else { return nil }
        if let schema = o["schema"] as? Int, schema > supportedSchema { return nil }
        var ask = Ask(id: id, kind: kind, sessionId: sessionId,
                      sessionName: (o["session_name"] as? String).flatMap { $0.isEmpty ? nil : $0 },
                      cwd: o["cwd"] as? String ?? "", createdAt: created, expiresAt: expires)
        switch kind {
        case .question:
            guard let raw = o["questions"] as? [Any], !raw.isEmpty else { return nil }
            let questions = raw.compactMap { ($0 as? [String: Any]).flatMap(question) }
            // Every question must be answerable, or the answer can't fit.
            guard questions.count == raw.count else { return nil }
            ask.questions = questions
        case .permission:
            guard let t = o["tool"] as? [String: Any], let name = t["name"] as? String else { return nil }
            ask.tool = Ask.Tool(name: name, preview: t["preview"] as? String ?? "")
        }
        return ask
    }

    static func question(_ q: [String: Any]) -> Ask.Question? {
        guard let text = q["question"] as? String, !text.isEmpty,
              let raw = q["options"] as? [Any] else { return nil }
        let options = raw.compactMap { item -> Ask.Option? in
            guard let o = item as? [String: Any], let label = o["label"] as? String, !label.isEmpty else { return nil }
            return Ask.Option(label: label, description: (o["description"] as? String).flatMap { $0.isEmpty ? nil : $0 })
        }
        guard !options.isEmpty, options.count == raw.count else { return nil }
        return Ask.Question(text: text, header: (q["header"] as? String).flatMap { $0.isEmpty ? nil : $0 },
                            options: options, multiSelect: q["multiSelect"] as? Bool ?? false)
    }
}

/// What the app hands the session.
public enum AskReply: Equatable, Sendable {
    /// The picked labels, one list per question, in question order.
    case choices([[String]])
    /// Your own words, for a single question.
    case text(String)
    case allow
    case deny

    /// `cc-moss ask answer <id> … --by desktop --json`. Labels go in as written; with
    /// several questions (or a label that itself starts with `N:`) each is
    /// written `N:label`, which the CLI reads by question number.
    public func arguments(id: String) -> [String] {
        var args = ["ask", "answer", id]
        switch self {
        case let .choices(perQuestion):
            let numbered = perQuestion.count > 1 || perQuestion.joined().contains(where: Self.looksNumbered)
            for (i, labels) in perQuestion.enumerated() {
                for label in labels {
                    args += ["--choice", numbered ? "\(i + 1):\(label)" : label]
                }
            }
        case let .text(text):
            args += ["--text", text]
        case .allow:
            args += ["--allow"]
        case .deny:
            args += ["--deny"]
        }
        return args + ["--by", "desktop", "--json"]
    }

    /// `2:x`: the CLI would take it for "question 2, option x".
    static func looksNumbered(_ label: String) -> Bool {
        guard let cut = label.firstIndex(of: ":") else { return false }
        return Int(label[..<cut].trimmingCharacters(in: .whitespaces)) != nil
    }
}

/// How an answer went, from `ask answer`'s exit status and first line.
public enum AskOutcome: Equatable, Sendable {
    /// The session took it and carries on.
    case answered
    /// Saved, but nothing was waiting for it right then (answered in the
    /// terminal a moment ago, most likely).
    case notWaiting
    /// Someone else answered first, or the session stopped waiting.
    case late
    /// cc-moss said no; its own words.
    case failed(String)

    /// Read `ask answer --json`: `{"schema":1,"result":…,"session":…}` on
    /// stdout, whatever the exit status (too late exits non-zero). No such
    /// line, or a result this app doesn't know: cc-moss refused, in its own
    /// words. The app ships with its CLI, so no older wording is read.
    public static func from(_ result: CommandResult) -> AskOutcome {
        struct Line: Decodable {
            var schema: Int
            var result: String
        }
        let out = String(decoding: result.stdout, as: UTF8.self)
        let line = out.split(whereSeparator: \.isNewline).last.map(String.init) ?? ""
        if let parsed = try? JSONDecoder().decode(Line.self, from: Data(line.utf8)), parsed.schema <= 1 {
            switch parsed.result {
            case "answered": return .answered
            case "not-waiting": return .notWaiting
            case "too-late": return .late
            default: return .failed(L.t("cc-moss said something this app doesn't know (\(parsed.result)). Update both together.",
                                        "cc-moss 回了一个这个 App 不认识的结果（\(parsed.result)），两个一起升级一下。"))
            }
        }
        let message = firstLine(result.stderr).map { $0.hasPrefix("cc-moss: ") ? String($0.dropFirst(9)) : $0 }
        return .failed(message ?? firstLine(out) ?? "exit \(result.status)")
    }
}

extension MossClient {
    /// The questions sessions are waiting on.
    public func asks() async throws -> [Ask] {
        try AskList.decode(try await plainData(["ask", "list", "--json"]))
    }

    /// Answer question `id`. `ask answer` waits up to 2 seconds to hear that
    /// the session took it, so this allows longer than most commands.
    public func answer(_ id: String, with reply: AskReply) async -> AskOutcome {
        do {
            let r = try await runner.run(executable, reply.arguments(id: id), timeout: max(timeout, 15))
            return AskOutcome.from(r)
        } catch {
            return .failed(L.t("cc-moss took too long to answer.", "cc-moss 太久没回话了。"))
        }
    }

    /// "I'm here": sessions wait a while for an answer from the app (90
    /// seconds per call).
    public func take(name: String) async throws {
        _ = try await plainData(["ask", "take", "--name", name])
    }
}
