import Foundation
import Testing
@testable import MossKit

@Suite struct FilterTests {
    let projects = Grouping.projects(sessions([
        sessionJSON(id: "A", name: "Login-Page", cwd: "/home/u/projects/acme", lastActive: "2026-10-16T11:00:00Z"),
        sessionJSON(id: "B", name: "job-b", cwd: "/home/u/projects/billing", lastActive: "2026-10-16T10:00:00Z", note: "Invoice script"),
        sessionJSON(id: "C", name: "job-c", cwd: "/home/u/projects/billing", lastActive: "2026-10-16T09:00:00Z", lastPrompt: "check the error codes"),
    ]), home: "/home/u")

    func ids(_ q: String) -> [String] {
        Filter.apply(q, to: projects).flatMap(\.sessions).map(\.id)
    }

    @Test func emptyQueryKeepsEverything() { #expect(ids("  ") == ["A", "B", "C"]) }
    @Test func matchesNameIgnoringCase() { #expect(ids("login") == ["A"]) }
    @Test func matchesProjectFolder() { #expect(ids("billing") == ["B", "C"]) }
    @Test func matchesNote() { #expect(ids("invoice") == ["B"]) }
    @Test func matchesLastPrompt() { #expect(ids("error codes") == ["C"]) }
    @Test func emptyProjectsDropped() { #expect(Filter.apply("login", to: projects).map(\.label) == ["acme"]) }

    @Test func stateFilterCombinesWithQuery() {
        let mixed = Grouping.projects(sessions([
            sessionJSON(id: "A", name: "job-a", status: "running", busy: true, lastActive: "2026-10-16T11:00:00Z"),
            sessionJSON(id: "B", name: "job-b", status: "interrupted", lastActive: "2026-10-16T10:00:00Z"),
            sessionJSON(id: "C", name: "other", status: "interrupted", lastActive: "2026-10-16T09:00:00Z"),
        ]), home: "/home/u")
        #expect(Filter.apply("", state: .interrupted, to: mixed).flatMap(\.sessions).map(\.id) == ["B", "C"])
        #expect(Filter.apply("job", state: .interrupted, to: mixed).flatMap(\.sessions).map(\.id) == ["B"])
    }

    @Test func countsByState() {
        let c = StateCounts(sessions([
            sessionJSON(id: "A", busy: true), sessionJSON(id: "B"), sessionJSON(id: "C"),
            sessionJSON(id: "D", status: "interrupted"), sessionJSON(id: "E", status: "closed"),
        ]), now: ISO8601DateFormatter().date(from: "2026-10-16T12:00:00Z")!)
        #expect(c == { var x = StateCounts([]); x.working = 1; x.waiting = 2; x.interrupted = 1; return x }())
    }
}
