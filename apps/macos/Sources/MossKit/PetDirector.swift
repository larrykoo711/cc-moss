import Foundation

/// How Mo gets your attention (Settings › General › Desktop Pet,
/// DESIGN-pet §6.5): hint first with the face and a move, talk once you
/// come over.
public enum PetInteraction: String, CaseIterable, Sendable {
    /// The face and a move only; click Mo for the message.
    case quiet
    /// The face, a move and a translucent catchphrase puff; hover for the message.
    case everyday
    /// The message box pops up straight away.
    case active

    public static let key = "petInteraction"

    public init(defaults: UserDefaults) {
        self = defaults.string(forKey: Self.key).flatMap(PetInteraction.init(rawValue:)) ?? .everyday
    }
}

/// What a message box is about: one kind of thing, for one or more sessions
/// (several of the same kind within 1.5 s become one message).
public struct PetMessage: Equatable, Sendable {
    public let kind: AlertKind
    /// One alert per session, oldest first.
    public private(set) var alerts: [SessionAlert]

    public init(_ alert: SessionAlert) {
        kind = alert.kind
        alerts = [alert]
    }

    public var slots: [String] { alerts.map(\.slotId) }

    /// The main button: what the notification would do (A23).
    public var action: AlertAction { kind.action }

    mutating func add(_ alert: SessionAlert) {
        if let i = alerts.firstIndex(where: { $0.slotId == alert.slotId }) {
            alerts[i] = alert
        } else {
            alerts.append(alert)
        }
    }

    mutating func keep(where relevant: (SessionAlert) -> Bool) {
        alerts = alerts.filter(relevant)
    }

    /// The words, given the sessions' display names (parallel to `alerts`).
    /// Never the session's content: our own plain words per kind and reason (§6.3).
    public func text(names: [String]) -> PetMessageText {
        let n = names.count
        let name = names.first ?? ""
        let list = Self.list(names)
        switch kind {
        case .needsYou:
            if n > 1 {
                return PetMessageText(title: L.t("\(n) sessions need you", "\(n) 个会话需要你"),
                                      body: L.t("\(list) are waiting for you.", "\(list) 都在等你。"),
                                      primary: L.t("Show", "看看"), secondary: L.t("Later", "稍后"))
            }
            let (title, body): (String, String) = switch alerts.first?.reason {
            case "question":
                (L.t("\(name) has a question", "\(name) 有个问题"),
                 L.t("It is waiting for you to pick an answer.", "它在等你选一个答案。"))
            case "input":
                (L.t("\(name) is waiting for you", "\(name) 在等你"),
                 L.t("It is waiting for your reply.", "它在等你回话。"))
            default:
                (L.t("\(name) needs you", "\(name) 需要你"),
                 L.t("Claude wants to do something and is waiting for your nod.", "Claude 想做一件事，在等你点头。"))
            }
            return PetMessageText(title: title, body: body, primary: L.t("Open", "去看看"), secondary: L.t("Later", "稍后"))
        case .interrupted:
            if n > 1 {
                return PetMessageText(title: L.t("\(n) sessions dropped", "有 \(n) 个会话断了"),
                                      body: L.t("Nothing is lost. Bring them back from the panel.", "进度都在，去面板里把它们找回来。"),
                                      primary: L.t("Show", "看看"), secondary: L.t("Not now", "先不"))
            }
            return PetMessageText(title: L.t("\(name) dropped", "\(name) 断了"),
                                  body: L.t("Nothing is lost. One click brings it back.", "别慌，进度都在。点一下就找回来。"),
                                  primary: L.t("Bring Back", "找回"), secondary: L.t("Not now", "先不"))
        case .restored:
            if n > 1 {
                return PetMessageText(title: L.t("All \(n) sessions are back", "\(n) 个会话都找回来了"),
                                      body: L.t("Civilizations carry on. So do conversations.", "文明会延续，对话也是。"),
                                      primary: L.t("Show", "看看"), secondary: L.t("Got it", "知道了"))
            }
            return PetMessageText(title: L.t("\(name) is back", "\(name) 找回来了"),
                                  body: L.t("Right where you left off.", "从上次停下的地方接着来。"),
                                  primary: L.t("Open", "去看看"), secondary: L.t("Got it", "知道了"))
        case .finished:
            if n > 1 {
                return PetMessageText(title: L.t("\(n) sessions are waiting", "\(n) 个会话在等你"),
                                      body: L.t("\(list) finished their turns.", "\(list) 这一轮都做完了。"),
                                      primary: L.t("Show", "看看"), secondary: L.t("Later", "稍后"))
            }
            return PetMessageText(title: L.t("\(name) is done for now", "\(name) 这一轮做完了"),
                                  body: L.t("Your move.", "轮到你了。"),
                                  primary: L.t("Open", "去看看"), secondary: L.t("Later", "稍后"))
        }
    }

    /// "a and b", "a, b and 2 more" / "a 和 b"、"a、b 等 4 个".
    static func list(_ names: [String]) -> String {
        switch names.count {
        case 0: return ""
        case 1: return names[0]
        case 2: return L.t("\(names[0]) and \(names[1])", "\(names[0]) 和 \(names[1])")
        default:
            return L.t("\(names[0]), \(names[1]) and \(names.count - 2) more", "\(names[0])、\(names[1]) 等 \(names.count) 个")
        }
    }
}

public struct PetMessageText: Equatable, Sendable {
    public var title: String
    public var body: String
    public var primary: String
    public var secondary: String
}

/// What Mo shows beside itself right now.
public enum PetDisplay: Equatable, Sendable {
    case nothing
    /// A translucent catchphrase; `id` changes with every new puff.
    case puff(id: Int, text: String)
    case box(PetMessage)
}

/// What a click on Mo should do.
public enum PetClick: Equatable, Sendable {
    /// Open the message box for what Mo is holding.
    case showBox
    /// Nothing to say: open the panel, as always (§7).
    case openPanel
}

/// Decides what Mo says, and when (DESIGN-pet §5, §6.5). Pure logic on a
/// clock the caller passes in: alerts the router gave Mo, the pointer, a
/// click, a dismissal in; what to show and when to look again out.
///
/// - One thing at a time: needs you > dropped > back > done; a higher one
///   takes over, a lower one is dropped (the panel and the icon still show it).
/// - The same kind within 1.5 s folds into the message without a second hint.
/// - The same session and kind within 60 s never hints again; it only joins
///   what Mo holds, there when you hover or click.
/// - Do not disturb: nothing at all (the face is T3.2's).
public struct PetDirector: Sendable {
    public static let mergeWindow: TimeInterval = 1.5
    public static let repeatWindow: TimeInterval = 60
    /// A puff fades out after this.
    public static let puffFor: TimeInterval = 4
    /// The box goes this long after the pointer leaves Mo and the box.
    public static let collapseAfter: TimeInterval = 1.5

    /// In active mode, how long a box popped up on its own stays when you
    /// never come near it; nil: until it's handled (§6.3).
    public static func popFor(_ kind: AlertKind) -> TimeInterval? {
        switch kind {
        case .needsYou: nil
        case .interrupted: 20
        case .restored: 5
        case .finished: 6
        }
    }

    /// How long Mo keeps a message for you; nil: until it's handled or no
    /// longer true (the session got its answer, or came back).
    public static func holdFor(_ kind: AlertKind) -> TimeInterval? {
        switch kind {
        case .needsYou, .interrupted: nil
        case .restored: 30
        case .finished: 60
        }
    }

    static func rank(_ kind: AlertKind) -> Int {
        switch kind {
        case .needsYou: 3
        case .interrupted: 2
        case .restored: 1
        case .finished: 0
        }
    }

    private struct Key: Hashable {
        var slot: String
        var kind: AlertKind
    }

    private struct Puff: Sendable {
        var id: Int
        var text: String
        var until: TimeInterval
    }

    private struct Held: Sendable {
        var message: PetMessage
        var mergeUntil: TimeInterval
        var expiresAt: TimeInterval?
        var puff: Puff?
        var box = false
        /// When an open box goes; nil: it stays.
        var closeAt: TimeInterval?

        func boxShown(at t: TimeInterval) -> Bool { box && (closeAt.map { t < $0 } ?? true) }
    }

    public var mode: PetInteraction
    public var doNotDisturb = false

    private var phrases: Catchphrases
    private var held: Held?
    private var onMo = false
    private var onBubble = false
    private var hinted: [Key: TimeInterval] = [:]
    private var puffs = 0

    public init(mode: PetInteraction = .everyday, phrases: Catchphrases = Catchphrases()) {
        self.mode = mode
        self.phrases = phrases
    }

    private var pointerOn: Bool { onMo || onBubble }

    /// The message Mo is holding, shown or not.
    public var message: PetMessage? { held?.message }

    // MARK: - Input

    /// An alert the router gave Mo. True when Mo should hint now (the face
    /// and a move); false when it was folded in, dropped or a repeat.
    @discardableResult
    public mutating func take(_ alert: SessionAlert, at t: TimeInterval) -> Bool {
        guard !doNotDisturb else { return false }
        expire(t)
        hinted = hinted.filter { t - $0.value < Self.repeatWindow }
        let key = Key(slot: alert.slotId, kind: alert.kind)
        let repeated = hinted[key] != nil

        if var current = held {
            if current.message.kind == alert.kind {
                current.message.add(alert)
                if let hold = Self.holdFor(alert.kind) { current.expiresAt = t + hold }
                if repeated {
                    held = current
                    return false
                }
                hinted[key] = t
                if t < current.mergeUntil {
                    held = current // the same moment: one hint for all of them
                    return false
                }
                current.mergeUntil = t + Self.mergeWindow
                express(&current, at: t)
                held = current
                return true
            }
            if Self.rank(alert.kind) < Self.rank(current.message.kind) { return false }
        }

        var next = Held(message: PetMessage(alert), mergeUntil: t + Self.mergeWindow,
                        expiresAt: Self.holdFor(alert.kind).map { t + $0 })
        // You are reading a box already: the new message takes its place.
        if let current = held, current.boxShown(at: t), pointerOn { next.box = true }
        if repeated {
            held = next
            return false
        }
        hinted[key] = t
        express(&next, at: t)
        held = next
        return true
    }

    /// The pointer went onto or off Mo.
    public mutating func pointer(onMo inside: Bool, at t: TimeInterval) {
        onMo = inside
        pointerMoved(at: t)
    }

    /// The pointer went onto or off the puff or the box.
    public mutating func pointer(onBubble inside: Bool, at t: TimeInterval) {
        onBubble = inside
        pointerMoved(at: t)
    }

    /// A single click on Mo.
    public mutating func click(at t: TimeInterval) -> PetClick {
        expire(t)
        guard !doNotDisturb, var current = held, !current.boxShown(at: t) else { return .openPanel }
        open(&current, at: t)
        held = current
        return .showBox
    }

    /// "Later", the close button, or the main action done: let it go.
    public mutating func dismiss() {
        held = nil
    }

    /// Drop the sessions whose alert is no longer true (it got its answer,
    /// it came back, it's gone); with none left, the message goes.
    public mutating func keep(where relevant: (SessionAlert) -> Bool) {
        guard var current = held else { return }
        current.message.keep(where: relevant)
        held = current.message.alerts.isEmpty ? nil : current
    }

    // MARK: - Output

    public func display(at t: TimeInterval) -> PetDisplay {
        guard !doNotDisturb, let current = held, !Self.expired(current, at: t) else { return .nothing }
        if current.boxShown(at: t) { return .box(current.message) }
        if let puff = current.puff, t < puff.until { return .puff(id: puff.id, text: puff.text) }
        return .nothing
    }

    /// When `display` changes by itself (a puff fades, a box closes, a
    /// message expires), so the caller can set one timer.
    public func nextChange(after t: TimeInterval) -> TimeInterval? {
        guard let current = held else { return nil }
        let times = [current.puff?.until, current.box ? current.closeAt : nil, current.expiresAt]
        return times.compactMap { $0 }.filter { $0 > t }.min()
    }

    // MARK: - Inside

    private static func expired(_ h: Held, at t: TimeInterval) -> Bool {
        h.expiresAt.map { t >= $0 } ?? false
    }

    private mutating func expire(_ t: TimeInterval) {
        if let current = held, Self.expired(current, at: t) { held = nil }
    }

    /// Mo's way of saying "something's up", by mode.
    private mutating func express(_ h: inout Held, at t: TimeInterval) {
        switch mode {
        case .quiet:
            break
        case .everyday:
            if h.boxShown(at: t) { return } // already reading: the box just changes
            if pointerOn {
                open(&h, at: t) // you are right there: no need to hint
                return
            }
            puffs += 1
            h.puff = Puff(id: puffs, text: phrases.next(), until: t + Self.puffFor)
        case .active:
            h.box = true
            h.puff = nil
            h.closeAt = pointerOn ? nil : Self.popFor(h.message.kind).map { t + $0 }
        }
    }

    private func open(_ h: inout Held, at t: TimeInterval) {
        h.box = true
        h.puff = nil
        h.closeAt = pointerOn ? nil : t + Self.collapseAfter
    }

    private mutating func pointerMoved(at t: TimeInterval) {
        expire(t)
        guard var current = held else { return }
        if pointerOn {
            if current.boxShown(at: t) {
                current.closeAt = nil // reading it: it stays
            } else if mode != .quiet, !doNotDisturb {
                open(&current, at: t) // you came over: Mo talks
            }
        } else if current.boxShown(at: t) {
            current.closeAt = t + Self.collapseAfter
        }
        held = current
    }
}
