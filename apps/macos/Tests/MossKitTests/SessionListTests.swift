import Foundation
import Testing
@testable import MossKit

@Suite struct SessionListTests {
    let list = sessions([
        sessionJSON(id: "A", name: "api", cwd: "/home/u/projects/acme"),
        sessionJSON(id: "B", name: "web", cwd: "/home/u/projects/acme"),
        sessionJSON(id: "C", name: "docs", cwd: "/home/u/projects/docs"),
    ])

    private func layout(pins: [String]) -> [SessionListItem] {
        let split = Pins(pins).split(list)
        return SessionList.items(pinned: split.pinned, projects: Grouping.projects(split.rest, home: "/home/u"),
                                 older: [], olderExpanded: false)
    }

    @Test func eachSessionOnceWithItsOwnId() {
        let items = layout(pins: ["B"])
        let ids = items.map(\.id)
        #expect(Set(ids).count == ids.count, "no id twice in the one list")
        #expect(ids == ["header:pinned", "B", "header:project:/home/u/projects/acme", "A", "header:project:/home/u/projects/docs", "C"])
    }

    @Test func aRowIsPinnedExactlyWhenItSitsInPinned() {
        for item in layout(pins: ["B", "C"]) {
            if case let .row(s, pinned) = item { #expect(pinned == ["B", "C"].contains(s.id), "\(s.id)") }
        }
    }

    @Test func aRowKeepsItsIdWhenItMoves() {
        let before = layout(pins: []).map(\.id)
        let after = layout(pins: ["C"]).map(\.id)
        #expect(before.contains("C") && after.contains("C"))
        #expect(after.filter { $0 == "C" }.count == 1)
        #expect(!after.contains("header:project:/home/u/projects/docs"), "an emptied folder loses its heading")
    }

    @Test func noPinsNoPinnedHeading() {
        #expect(!layout(pins: []).contains(.pinnedHeader))
    }
}
