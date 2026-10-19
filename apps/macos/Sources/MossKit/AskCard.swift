import Foundation

/// One question's card, in the panel or in Mo's bubble: what is picked so
/// far, and whether the answer is on its way. Answering the wrong thing
/// sends the session off the wrong way, so every step is deliberate:
/// several questions are answered one by one and sent together, a multiple
/// choice waits for "Done", and nothing goes twice.
public struct AskCard: Equatable, Sendable {
    public enum Phase: Equatable, Sendable {
        case choosing
        /// `ask answer` is running; the buttons are off.
        case submitting
        /// Answered, too late, or nobody waiting: the card says so, then goes.
        case finished(AskOutcome)
    }

    public private(set) var ask: Ask
    /// The question on show; with several, `questions.count` is the review step.
    public private(set) var step = 0
    /// The labels picked, one list per question.
    public private(set) var picks: [[String]]
    /// Changing one answer from the review: the next pick goes back there.
    private var revisiting = false
    /// "Type something" is open on the question shown.
    public var writing = false
    /// What's typed, per question (kept when you move between them).
    private var drafts: [String]
    /// Questions answered in your own words, per question.
    private var typedAnswers: [String?]
    /// What's typed for the question shown.
    public var draft: String {
        get { drafts.indices.contains(step) ? drafts[step] : "" }
        set { if drafts.indices.contains(step) { drafts[step] = newValue } }
    }
    public private(set) var phase: Phase = .choosing
    /// What was sent, once it was.
    public private(set) var sent: AskReply?
    /// cc-moss refused the last try; the card stays, with its words.
    public private(set) var problem: String?

    public init(_ ask: Ask) {
        self.ask = ask
        picks = Array(repeating: [], count: ask.questions.count)
        drafts = Array(repeating: "", count: max(ask.questions.count, 1))
        typedAnswers = Array(repeating: nil, count: ask.questions.count)
    }

    /// Question `i` answered in your own words, if it was.
    public func typed(_ i: Int) -> String? {
        typedAnswers.indices.contains(i) ? typedAnswers[i] : nil
    }

    public var id: String { ask.id }
    public var count: Int { ask.questions.count }
    public var several: Bool { count > 1 }
    /// The question on show; nil for a permission or on the review step.
    public var question: Ask.Question? { ask.questions.indices.contains(step) ? ask.questions[step] : nil }
    /// Every question answered: what you picked, before it goes.
    public var reviewing: Bool { several && step == count }
    /// A question can be answered in your own words (`--text`, by number
    /// when there are several).
    public var canWrite: Bool { ask.kind == .question }
    /// The last answer row, "Type something", as Claude Code's own list
    /// ends: on every question while it's being answered, not the review.
    public var offersOwnWords: Bool { canWrite && phase == .choosing && question != nil }
    public var submitting: Bool { phase == .submitting }

    public func picked(_ label: String) -> Bool {
        picks.indices.contains(step) && picks[step].contains(label)
    }

    /// A newer copy of the same question (the session's name filled in).
    mutating func refresh(_ newer: Ask) {
        guard newer.id == ask.id, newer.kind == ask.kind, newer.questions == ask.questions else { return }
        ask = newer
    }

    // MARK: - Choosing

    /// A click on an option. Single choice: that's the answer to this
    /// question; the last (or only) one sends. Multiple choice: on or off.
    /// Returns the reply to send, if this click sends one.
    public mutating func choose(_ label: String) -> AskReply? {
        guard phase == .choosing, let q = question, q.options.contains(where: { $0.label == label }) else { return nil }
        problem = nil
        if q.multiSelect {
            if let i = picks[step].firstIndex(of: label) { picks[step].remove(at: i) } else { picks[step].append(label) }
            // In the order the session listed them, whatever the click order.
            picks[step] = q.options.map(\.label).filter(picks[step].contains)
            return nil
        }
        picks[step] = [label]
        if typedAnswers.indices.contains(step) { typedAnswers[step] = nil }
        writing = false
        return advance()
    }

    /// "Done" under a multiple choice.
    public mutating func confirm() -> AskReply? {
        guard phase == .choosing, let q = question, q.multiSelect, !picks[step].isEmpty else { return nil }
        return advance()
    }

    /// One question back (from the review step too).
    public mutating func back() {
        guard phase == .choosing, step > 0 else { return }
        problem = nil
        revisiting = false
        writing = typed(step - 1) != nil
        step -= 1
    }

    /// From the review, open question `i` to change its answer; answering it
    /// comes straight back to the review.
    public mutating func revisit(_ i: Int) {
        guard phase == .choosing, reviewing, ask.questions.indices.contains(i) else { return }
        problem = nil
        step = i
        revisiting = true
        writing = typed(i) != nil
    }

    /// "Send" on the review step.
    public mutating func sendAll() -> AskReply? {
        guard reviewing, picks.indices.allSatisfy({ !picks[$0].isEmpty || typed($0) != nil }) else { return nil }
        guard typedAnswers.contains(where: { $0 != nil }) else { return submit(.choices(picks)) }
        return submit(.answers(picks.indices.map { i in typed(i).map(AskReply.Answer.typed) ?? .picked(picks[i]) }))
    }

    /// "Type something": the row turns into a field.
    public mutating func startWriting() {
        guard offersOwnWords else { return }
        writing = true
    }

    /// Esc in the field: back to the answers, the draft kept for next time.
    public mutating func stopWriting() {
        writing = false
    }

    /// ⌘↩ in the field: a single question is answered and sent; one of
    /// several is answered in these words, and the next question (or the
    /// review) comes up.
    public mutating func sendText() -> AskReply? {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard canWrite, phase == .choosing, question != nil, !text.isEmpty else { return nil }
        guard several else { return submit(.text(text)) }
        typedAnswers[step] = text
        picks[step] = []
        writing = false
        return advance()
    }

    public mutating func allow() -> AskReply? {
        guard ask.kind == .permission else { return nil }
        return submit(.allow)
    }

    public mutating func deny() -> AskReply? {
        guard ask.kind == .permission else { return nil }
        return submit(.deny)
    }

    private mutating func advance() -> AskReply? {
        if !several { return submit(.choices(picks)) }
        step = revisiting ? count : step + 1
        revisiting = false
        return nil
    }

    private mutating func submit(_ reply: AskReply) -> AskReply? {
        guard phase == .choosing else { return nil }
        problem = nil
        phase = .submitting
        sent = reply
        return reply
    }

    // MARK: - The answer's fate

    /// What `ask answer` said. A refusal leaves the card answerable, with
    /// cc-moss's reason on it; anything else closes it.
    public mutating func finish(_ outcome: AskOutcome) {
        guard phase == .submitting else { return }
        if case let .failed(message) = outcome {
            phase = .choosing
            problem = message
            return
        }
        phase = .finished(outcome)
    }
}

/// Every question card the app shows, kept in step with `ask list`.
public struct AskDesk: Equatable, Sendable {
    public private(set) var cards: [AskCard] = []
    /// The card a notification or Mo pointed you to: drawn first, ringed.
    public var focused: String?
    /// The ids in the last `ask list`.
    private var standing: Set<String> = []

    public init() {}

    /// What changed with a fresh `ask list`.
    public struct Change: Equatable, Sendable {
        /// Questions seen for the first time: tell you about these.
        public var arrived: [Ask] = []
        /// Questions no longer standing (answered in the terminal, timed out,
        /// the session ended, or answered here): take back what was said about them.
        public var gone: [String] = []

        public init(arrived: [Ask] = [], gone: [String] = []) {
            self.arrived = arrived
            self.gone = gone
        }

        public var isEmpty: Bool { arrived.isEmpty && gone.isEmpty }
    }

    /// Cards still being answered go when their question does; one that
    /// was just sent stays to say how it went, until `clear`.
    @discardableResult
    public mutating func update(_ asks: [Ask]) -> Change {
        var change = Change()
        let ids = Set(asks.map(\.id))
        change.gone = standing.subtracting(ids).sorted()
        cards.removeAll { !ids.contains($0.id) && $0.phase == .choosing }
        for ask in asks {
            if let i = cards.firstIndex(where: { $0.id == ask.id }) {
                cards[i].refresh(ask)
            } else if !standing.contains(ask.id) {
                cards.append(AskCard(ask))
                change.arrived.append(ask)
            }
        }
        standing = ids
        if let focused, !cards.contains(where: { $0.id == focused }) { self.focused = nil }
        return change
    }

    /// Cards in the order they are drawn: the one you were pointed to first,
    /// then oldest first (the longest wait is the most urgent).
    public var ordered: [AskCard] {
        guard let focused, let card = cards.first(where: { $0.id == focused }) else { return cards }
        return [card] + cards.filter { $0.id != focused }
    }

    /// Still waiting on you (not sent, not done).
    public var open: [AskCard] { cards.filter { $0.phase != .submitting && !Self.finished($0) } }

    public func card(_ id: String) -> AskCard? { cards.first { $0.id == id } }

    /// Change one card; returns what to send, if anything.
    public mutating func edit(_ id: String, _ body: (inout AskCard) -> AskReply?) -> AskReply? {
        guard let i = cards.firstIndex(where: { $0.id == id }) else { return nil }
        return body(&cards[i])
    }

    public mutating func finish(_ id: String, _ outcome: AskOutcome) {
        guard let i = cards.firstIndex(where: { $0.id == id }) else { return }
        cards[i].finish(outcome)
    }

    /// The card said its piece: take it away.
    public mutating func clear(_ id: String) {
        cards.removeAll { $0.id == id }
        if focused == id { focused = nil }
    }

    /// Nothing more to show: `ask list` failed or the feature is off. The
    /// questions count as gone, so what was said about them is taken back.
    @discardableResult
    public mutating func reset() -> Change {
        update([])
    }

    static func finished(_ card: AskCard) -> Bool {
        if case .finished = card.phase { return true }
        return false
    }

    /// Sessions waiting for you to type something (Claude Code's "waiting
    /// for your input"), which have no question open and can be typed into.
    /// Moss is left out: waiting is what it does all day.
    public static func waitingForInput(_ sessions: [Session], asks: [Ask]) -> [Session] {
        let asking = Set(asks.map(\.sessionId))
        return sessions.filter {
            $0.attention == "input" && !$0.isMoss && Message.allowed($0) && !asking.contains($0.sessionId)
        }
    }

    /// The name the panel shows for the session that asked.
    public static func sessionTitle(_ ask: Ask, sessions: [Session]) -> String {
        if let s = sessions.first(where: { $0.sessionId == ask.sessionId }) { return s.title }
        if let name = ask.sessionName { return name }
        return String(ask.sessionId.prefix(8))
    }

    /// The panel's id for the session that asked, when it lists it.
    public static func slot(_ ask: Ask, sessions: [Session]) -> String? {
        sessions.first { $0.sessionId == ask.sessionId }?.id
    }
}
