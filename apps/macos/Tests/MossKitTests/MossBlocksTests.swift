import Foundation
import Testing
@testable import MossKit

@Suite struct MossBlocksTests {
    private func blocks(_ text: String) -> [MossBlock] { MossMarkdown.blocks(text) }

    @Test func paragraphsSplitOnBlankLines() {
        #expect(blocks("one\ntwo\n\nthree") == [.paragraph("one\ntwo"), .paragraph("three")])
    }

    @Test func headings() {
        #expect(blocks("## 今天\nbody") == [.heading(2, "今天"), .paragraph("body")])
        #expect(blocks("#nospace") == [.paragraph("#nospace")], "needs a space to be a heading")
    }

    @Test func bulletListsWithNesting() {
        #expect(blocks("- **a**：x\n* b\n  - c") == [.list(ordered: false, items: [
            .init(level: 0, marker: nil, text: "**a**：x"),
            .init(level: 0, marker: nil, text: "b"),
            .init(level: 1, marker: nil, text: "c"),
        ])])
    }

    @Test func orderedListsKeepTheirNumbers() {
        #expect(blocks("1. a\n2) b\n10. c") == [.list(ordered: true, items: [
            .init(level: 0, marker: "1.", text: "a"),
            .init(level: 0, marker: "2.", text: "b"),
            .init(level: 0, marker: "10.", text: "c"),
        ])])
    }

    @Test func aListStopsAtTextAfterIt() {
        #expect(blocks("- a\n\nafter") == [.list(ordered: false, items: [.init(level: 0, marker: nil, text: "a")]),
                                          .paragraph("after")])
        #expect(blocks("intro:\n- a") == [.paragraph("intro:"),
                                         .list(ordered: false, items: [.init(level: 0, marker: nil, text: "a")])])
    }

    @Test func quotesAndRules() {
        #expect(blocks("> one\n> two\n\n---\nx") == [.quote("one\ntwo"), .rule, .paragraph("x")])
    }

    @Test func codeFencesKeepTheirInsides() {
        #expect(blocks("```sh\n- not a list\n# not a heading\n```") == [.code("- not a list\n# not a heading")])
        #expect(blocks("```\nopen to the end") == [.code("open to the end")])
    }

    @Test func tables() {
        let text = "| 会话 | 状态 |\n|---|:---:|\n| api | 断了 |\n| docs | 做完了 |"
        #expect(blocks(text) == [.table(header: ["会话", "状态"], rows: [["api", "断了"], ["docs", "做完了"]])])
    }

    @Test func aPipeLineWithoutASeparatorIsText() {
        #expect(blocks("a | b") == [.paragraph("a | b")])
    }

    @Test func emptyTextIsNothing() {
        #expect(blocks("\n\n").isEmpty)
    }
}
