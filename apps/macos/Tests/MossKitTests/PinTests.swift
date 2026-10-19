import Foundation
import Testing
@testable import MossKit

@Suite struct PinTests {
    let list = sessions([
        sessionJSON(id: "A", name: "api"),
        sessionJSON(id: "B", name: "web", status: "interrupted"),
        sessionJSON(id: "C", name: "docs"),
        sessionJSON(id: "D", name: "old", status: "closed"),
    ])

    @Test func theNewestPinComesFirst() {
        var pins = Pins()
        pins.toggle("A")
        pins.toggle("C")
        #expect(pins.ids == ["C", "A"])
        #expect(pins.split(list).pinned.map(\.id) == ["C", "A"])
    }

    @Test func pinnedRowsAreNotRepeatedInTheirFolders() {
        let split = Pins(["C"]).split(list)
        #expect(split.rest.map(\.id) == ["A", "B", "D"])
    }

    @Test func toggleUnpins() {
        var pins = Pins(["A", "C"])
        pins.toggle("A")
        #expect(pins.ids == ["C"])
        #expect(!pins.contains("A"))
    }

    @Test func pinningKeepsTheState() {
        let pinned = Pins(["B"]).split(list).pinned
        #expect(pinned.first?.state == .interrupted, "a pinned dropped session still counts as dropped")
    }

    @Test func goneOrClosedPinsAreDropped() {
        let pins = Pins(["X", "D", "A"]).pruned(keeping: list)
        #expect(pins.ids == ["A"])
        #expect(Pins(["D"]).split(list).pinned.isEmpty)
    }

    @Test func duplicatesCollapse() {
        #expect(Pins(["A", "A", "C"]).ids == ["A", "C"])
    }

    @Test func theyRoundTrip() throws {
        let defaults = try #require(UserDefaults(suiteName: "pin-tests-\(UUID().uuidString)"))
        Pins(["C", "A"]).save(defaults)
        #expect(Pins.load(defaults).ids == ["C", "A"])
    }

    @Test func theRowButtonsGoMessageOpenRename() {
        #expect(RowActions(list[0], hopeless: false).inline == [.message, .open, .rename])
    }
}
