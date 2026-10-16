import Foundation
import Testing
@testable import MossKit

@Suite struct GroupingTests {
    @Test func projectsOrderedByMostRecentActivity() {
        let list = sessions([
            sessionJSON(id: "A", cwd: "/home/u/projects/acme", lastActive: "2026-10-16T09:00:00Z"),
            sessionJSON(id: "B", cwd: "/home/u/projects/billing", lastActive: "2026-10-16T11:00:00Z"),
            sessionJSON(id: "C", cwd: "/home/u/projects/acme", lastActive: "2026-10-16T10:00:00Z"),
        ])
        let projects = Grouping.projects(list, home: "/home/u")
        #expect(projects.map(\.label) == ["billing", "acme"])
        #expect(projects[1].sessions.map(\.id) == ["C", "A"])
    }

    @Test func duplicateBasenamesUseShortPath() {
        let list = sessions([
            sessionJSON(id: "A", cwd: "/home/u/work/api", lastActive: "2026-10-16T11:00:00Z"),
            sessionJSON(id: "B", cwd: "/home/u/play/api", lastActive: "2026-10-16T10:00:00Z"),
            sessionJSON(id: "C", cwd: "/home/u", lastActive: "2026-10-16T09:00:00Z"),
        ])
        #expect(Grouping.projects(list, home: "/home/u").map(\.label) == ["~/work/api", "~/play/api", "~"])
    }

    @Test func closedSessionsHidden() {
        let list = sessions([sessionJSON(id: "A", status: "closed"), sessionJSON(id: "B")])
        #expect(Grouping.projects(list, home: "/home/u").flatMap(\.sessions).map(\.id) == ["B"])
    }

    @Test func shortPathCollapsesOnlyLongPaths() {
        #expect(Grouping.shortPath("/srv/a/b/c", home: "/home/u") == "/srv/a/b/c")
        #expect(Grouping.shortPath("/srv/somewhere/rather/deep/inside/api", home: "/home/u") == "…/inside/api")
    }
}
