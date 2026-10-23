import Foundation
import Testing
@testable import MossKit

@Suite struct NewSessionTests {
    let list = sessions([
        sessionJSON(id: "A", name: "login", status: "running", busy: true, cwd: "/home/u/projects/acme", lastActive: "2026-10-16T09:00:00Z"),
        sessionJSON(id: "B", name: "invoice", status: "interrupted", cwd: "/home/u/projects/billing", lastActive: "2026-10-16T08:00:00Z"),
        sessionJSON(id: "C", name: "docs", status: "running", cwd: "/home/u/projects/docs", lastActive: "2026-10-16T11:00:00Z"),
        sessionJSON(id: "D", name: "review", status: "running", cwd: "/home/u/projects/acme", lastActive: "2026-10-16T10:00:00Z"),
        sessionJSON(id: "E", name: "gone", status: "closed", cwd: "/home/u/projects/old", lastActive: "2026-10-16T07:00:00Z"),
    ])

    @Test func buildsCommand() {
        #expect(NewSession.arguments(name: "my job", directory: "/home/u/a b")
            == ["new", "my job", "-C", "/home/u/a b", "--detach", "--json"])
        #expect(!NewSession.arguments(name: "x", directory: "/d").contains("--yes"))
        #expect(NewSession.plan(name: "  fresh ", directory: "/home/u/projects/acme", sessions: list)
            == .create(["new", "fresh", "-C", "/home/u/projects/acme", "--detach", "--json"]))
    }

    @Test func noDirectoryIsSelectedWithoutBackendApproval() {
        var selection = WorkingDirectorySelection()
        #expect(selection.canonical == nil)
        _ = selection.begin("/home/u/work")
        #expect(selection.canonical == nil)
        selection.cancel()
        #expect(selection.canonical == nil)
    }

    @Test func defaultsToRecentProject() {
        #expect(NewSession.recentProjects([]).isEmpty)
        #expect(NewSession.recentProjects(list) == [
            "/home/u/projects/docs", "/home/u/projects/acme", "/home/u/projects/billing", "/home/u/projects/old",
        ])
        #expect(NewSession.recentProjects(list, limit: 2).count == 2)
        #expect(NewSession.defaultName(for: "/home/u/projects/docs") == "docs")
    }

    @Test func sameNameRunningCreatesFreshIdentity() {
        let plan = NewSession.plan(name: "login", directory: "/anywhere", sessions: list)
        #expect(plan == .create(NewSession.arguments(name: "login", directory: "/anywhere")))
        #expect(NewSession.hint(name: "login", sessions: list) == .running(list[0]))
    }

    @Test func sameNameInterruptedCreatesFreshIdentity() {
        #expect(NewSession.plan(name: "invoice", directory: "/anywhere", sessions: list)
            == .create(NewSession.arguments(name: "invoice", directory: "/anywhere")))
        // A closed session does not hold its name.
        #expect(NewSession.hint(name: "gone", sessions: list) == .free)
    }

    @Test func aTakenNameGetsTheNextFreeNumber() {
        #expect(NewSession.freeName("fresh", sessions: list) == "fresh")
        #expect(NewSession.freeName("docs", sessions: list) == "docs-2")
        // A stopped session still holds its name; a closed one doesn't.
        #expect(NewSession.freeName("invoice", sessions: list) == "invoice-2")
        #expect(NewSession.freeName("gone", sessions: list) == "gone")
        let more = list + sessions([sessionJSON(id: "F", name: "docs-2", status: "running")])
        #expect(NewSession.freeName("docs", sessions: more) == "docs-3")
        #expect(NewSession.freeName(" docs ", sessions: list) == "docs-2", "spaces around don't count")
    }

    @Test func refusesNamesCcMossWouldMisread() {
        guard case .invalid = NewSession.plan(name: "-x", directory: "/d", sessions: []) else {
            Issue.record("a leading dash must be refused"); return
        }
        guard case .invalid = NewSession.hint(name: "   ", sessions: []) else {
            Issue.record("an empty name must be refused"); return
        }
    }
}

@Suite struct NoteTests {
    @Test func noteTextIsOneArgument() async throws {
        let text = #"--goal it's "done"; rm -rf ~ $(x)"#
        #expect(Note.arguments(target: "job-a", text: text) == ["note", "job-a", "--", text])
        let runner = RecordingRunner([])
        try await MossClient(executable: "/x/cc-moss", runner: runner).setNote(target: "job-a", text: " hi there ")
        #expect(await runner.calls == [["note", "job-a", "--", "hi there"]])
    }

    @Test func emptyClears() {
        #expect(Note.arguments(target: "job-a", text: "  \n") == ["note", "job-a", "--", ""])
        #expect(Note.current(sessions([sessionJSON(note: "keep")])[0]) == "keep")
        #expect(Note.current(sessions([sessionJSON()])[0]) == "")
    }
}

extension CloseTests {
    @Test func asksFirst() {
        let s = sessions([sessionJSON(id: "A", name: "job-a", status: "interrupted")])
        var gate = Confirmation<[String]>()
        gate.request(s.map(\.target))
        #expect(gate.asking, "nothing runs on the first click")
        gate.cancel()
        #expect(gate.confirm() == nil, "cancelled: nothing to run")
        gate.request(s.map(\.target))
        #expect(gate.confirm() == ["A-sid-0000"])
        #expect(gate.confirm() == nil, "runs once")
        #expect(Close.question(s).contains("job-a"))
    }

    @Test func onlyInterruptedSessionsCanBeDismissed() {
        let s = sessions([sessionJSON(id: "A", status: "interrupted"), sessionJSON(id: "B", status: "running")])
        #expect(Close.allowed(s[0]))
        #expect(!Close.allowed(s[1]))
    }
}
