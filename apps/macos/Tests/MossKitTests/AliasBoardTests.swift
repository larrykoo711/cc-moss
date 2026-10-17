import Testing
@testable import MossKit

@Suite struct AliasBoardTests {
    @Test func aliasesLandOnTheirTasks() {
        let board = AliasBoard.board([
            Alias(name: "t", command: "pick"),
            Alias(name: "mycc", command: "new  --dangerously-skip-permissions"),
            Alias(name: "tl", command: "pick --all"),
        ])
        #expect(board.rows.first { $0.task.id == "pick" }?.alias?.name == "t")
        #expect(board.rows.first { $0.task.id == "trusted" }?.alias?.name == "mycc", "spaces don't matter")
        #expect(board.rows.first { $0.task.id == "new" }?.alias == nil)
        #expect(board.custom.map(\.name) == ["tl"])
    }

    @Test func aSecondAliasForATaskIsCustom() {
        let board = AliasBoard.board([Alias(name: "t", command: "pick"), Alias(name: "p", command: "pick")])
        #expect(board.rows.first?.alias?.name == "t")
        #expect(board.custom.map(\.name) == ["p"])
    }

    @Test func savingTheField() {
        let t = Alias(name: "t", command: "pick")
        #expect(AliasBoard.change(field: "", current: nil) == .nothing)
        #expect(AliasBoard.change(field: " tn ", current: nil) == .add(name: "tn"))
        #expect(AliasBoard.change(field: "t", current: t) == .nothing)
        #expect(AliasBoard.change(field: "tp", current: t) == .rename(from: "t", to: "tp"))
        #expect(AliasBoard.change(field: "  ", current: t) == .remove(name: "t"))
    }

    @Test func customIsOneLine() {
        #expect(AliasBoard.parseCustom("tl = pick --all")! == ("tl", "pick --all"))
        #expect(AliasBoard.parseCustom("tl=cc-moss pick --all")! == ("tl", "pick --all"))
        #expect(AliasBoard.parseCustom("tl pick") == nil)
        #expect(AliasBoard.parseCustom("t l = pick") == nil)
        #expect(AliasBoard.parseCustom(" = pick") == nil)
    }
}
