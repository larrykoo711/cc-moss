import Foundation

/// Moving a session that runs outside tmux into tmux (`cc-moss adopt`): its
/// process in the old terminal ends and it carries on in tmux, the
/// conversation kept. A session without a real name gets one from its
/// folder on the way, and its summary is read again from its transcript.
public enum Adopt {
    /// Whether a row offers "Move into tmux".
    public enum Availability: Equatable, Sendable {
        /// Already in tmux, not running, or Moss: no button.
        case none
        case ready
        /// Mid-turn: the button is there but waits.
        case working
        /// It's asking you something in its terminal: answer that first.
        case waitingOnYou
    }

    public static func availability(_ s: Session) -> Availability {
        guard !s.isMoss, !s.inTmux, s.status == "running" else { return .none }
        if s.busy { return .working }
        if s.attention != nil { return .waitingOnYou }
        return .ready
    }

    /// Before asking: will it be tidied up too? Then the button says "Tidy up
    /// and move into tmux". `named` is the name cc-moss said it will give.
    public static func tidies(_ s: Session, named: String? = nil) -> Bool {
        named != nil || looksAutoNamed(s) || s.subtitle == nil
    }

    /// No real name (what cc-moss calls auto-named): none, something that
    /// looks like an id (8+ hex characters, a digit before any "-"), or the
    /// start of its conversation id.
    static func looksAutoNamed(_ s: Session) -> Bool {
        guard let name = s.name?.trimmingCharacters(in: .whitespaces), !name.isEmpty else { return true }
        guard name.allSatisfy({ $0.isHexDigit || $0 == "-" }) else { return false }
        let head = name.prefix { $0 != "-" }
        if name.count >= 8, head.contains(where: \.isNumber) { return true }
        return name.count >= 6 && s.sessionId.hasPrefix(name)
    }

    /// What `cc-moss adopt <name> --json [--dry-run]` said.
    public enum Outcome: Equatable, Sendable {
        /// `--dry-run`: it can be moved; `named` is the name it will get.
        case wouldMove(named: String?)
        case moved(named: String?, tmuxSession: String?)
        /// cc-moss won't move it (`reason`: busy, in-tmux, not-running, …),
        /// with its words for why.
        case refused(reason: String, message: String)
        case failed(message: String)
    }

    private struct Wire: Decodable {
        var schema: Int
        var result: String
        var reason: String?
        var message: String?
        var named: String?
        var tmux_session: String?
    }

    public static func outcome(_ data: Data) throws -> Outcome {
        let wire: Wire
        do {
            wire = try JSONDecoder().decode(Wire.self, from: data)
        } catch {
            throw MossClient.Failure.unreadable("\(error)")
        }
        guard wire.schema <= 1 else { throw MossClient.Failure.newerSchema(wire.schema) }
        let named = wire.named.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 }
        let message = wire.message ?? ""
        switch wire.result {
        case "would-move": return .wouldMove(named: named)
        case "moved": return .moved(named: named, tmuxSession: wire.tmux_session)
        case "refused": return .refused(reason: wire.reason ?? "", message: message)
        default: return .failed(message: message)
        }
    }

    public static func arguments(target: String, dryRun: Bool) -> [String] {
        ["adopt", target, "--json"] + (dryRun ? ["--dry-run"] : [])
    }
}

extension MossClient {
    /// `cc-moss adopt <target> --json [--dry-run]`. A refusal or a failure
    /// exits 1 but still answers in JSON, so stdout is read either way.
    /// Moving waits for the old process to quit (up to 10 s) and the new one
    /// to start, hence the long wait.
    public func adopt(_ target: String, dryRun: Bool = false) async throws -> Adopt.Outcome {
        let r: CommandResult
        do {
            r = try await runner.run(executable, Adopt.arguments(target: target, dryRun: dryRun), timeout: dryRun ? timeout : 60)
        } catch RunError.timedOut {
            throw Failure.timedOut
        } catch {
            throw Failure.command("\(error)")
        }
        if let outcome = try? Adopt.outcome(r.stdout) { return outcome }
        guard r.status == 0 else {
            throw Failure.command(firstLine(r.stderr) ?? firstLine(String(decoding: r.stdout, as: UTF8.self)) ?? "exit \(r.status)")
        }
        return try Adopt.outcome(r.stdout)
    }
}
