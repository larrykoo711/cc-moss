import Foundation
import Testing
@testable import MossKit

@Suite struct MessageTests {
    let list = sessions([
        sessionJSON(id: "A", name: "job-a", status: "running", busy: false),
        sessionJSON(id: "B", name: "job-b", status: "running", busy: true),
        sessionJSON(id: "C", name: "old", status: "closed"),
        sessionJSON(id: "D", name: "far", status: "running", tmux: false),
    ])

    @Test func textIsOneArgumentAfterDoubleDash() {
        #expect(Message.arguments(target: "job-a", text: "line one\nline 'two'") == ["send", "job-a", "--", "line one\nline 'two'"])
    }

    @Test func onlyRunningSessionsInTmux() {
        #expect(Message.allowed(list[0]) && Message.allowed(list[1]))
        #expect(!Message.allowed(list[2]))
        #expect(!Message.allowed(list[3]))
    }

    @Test func renameIsClaudesOwnCommand() {
        #expect(Message.renameText("  login-page ") == "/rename login-page")
    }

    @Test func renameChecks() {
        #expect(Message.checkRename("", of: list[0], among: list) == .empty)
        #expect(Message.checkRename("job-a", of: list[0], among: list) == .unchanged)
        #expect(Message.checkRename("job-b", of: list[0], among: list) == .taken("job-b"))
        #expect(Message.checkRename("old", of: list[0], among: list) == .ok, "a closed session's name is free")
        #expect(Message.checkRename("a\u{3}b", of: list[0], among: list) == .badCharacters)
        #expect(Message.checkRename("new-name", of: list[0], among: list) == .ok)
    }
}
