import Foundation
import Testing
@testable import MossKit

@Suite struct SessionSearchTests {
    let list = sessions([
        sessionJSON(id: "A", name: "api-review", cwd: "/home/u/projects/shop", lastActive: "2026-10-16T09:00:00.000Z"),
        sessionJSON(id: "B", name: "mac-dev-setup", cwd: "/home/u/projects/infra", lastActive: "2026-10-16T08:00:00.000Z"),
        sessionJSON(id: "C", name: "接口评审", cwd: "/home/u/projects/shop", lastActive: "2026-10-16T07:00:00.000Z"),
        sessionJSON(id: "D", name: "rapid", cwd: "/home/u/projects/web", lastActive: "2026-10-16T10:00:00.000Z"),
        sessionJSON(id: "E", name: "docs", cwd: "/home/u/api-gateway/docs", lastActive: "2026-10-16T06:00:00.000Z"),
        sessionJSON(id: "F", name: "notes", cwd: "/home/u/projects/misc", lastActive: "2026-10-16T05:00:00.000Z",
                    note: "check the api limits"),
        sessionJSON(id: "G", name: "fixInvoiceTotals", cwd: "/home/u/projects/billing", lastActive: "2026-10-16T04:00:00.000Z",
                    lastPrompt: "why does the API time out?"),
    ])

    var index: SearchIndex { SearchIndex(list) }
    private func ids(_ q: String) -> [String] { index.search(q).map(\.session.id) }

    @Test func nameFirstThenFolderThenNoteThenPrompt() {
        // "api": a name prefix, a name containing it, a path, a note, a prompt.
        #expect(ids("api") == ["A", "D", "E", "F", "G"])
        let hits = index.search("api")
        #expect(hits.map(\.rank) == [SearchRank.namePrefix, SearchRank.nameContains, SearchRank.folder, SearchRank.note, SearchRank.prompt])
    }

    @Test func tiesGoToTheMostRecent() {
        let two = SearchIndex(sessions([
            sessionJSON(id: "old", name: "web-old", lastActive: "2026-10-16T01:00:00.000Z"),
            sessionJSON(id: "new", name: "web-new", lastActive: "2026-10-16T09:00:00.000Z"),
        ]))
        #expect(two.search("web").map(\.session.id) == ["new", "old"])
    }

    @Test func theNameIsHighlighted() {
        let hit = index.search("api")
        #expect(hit[0].nameRanges == [0..<3])
        #expect(hit[1].nameRanges == [1..<4], "r[api]d")
        #expect(index.search("review").first?.nameRanges == [4..<10], "a word start")
    }

    @Test func whyItMatchedWhenTheNameDoesnt() {
        let hits = Dictionary(uniqueKeysWithValues: index.search("api").map { ($0.session.id, $0.reason) })
        #expect(hits["A"] == .some(nil))
        #expect(hits["E"] == .some(.path("api")))
        #expect(hits["F"] == .some(.note("check the api limits")))
        if case .prompt(let text)?? = hits["G"] { #expect(text.contains("API")) } else { Issue.record("no prompt reason") }
    }

    @Test func initialsFindDashedAndCamelNames() {
        #expect(ids("mds") == ["B"])
        #expect(index.search("mds").first?.nameRanges == [0..<1, 4..<5, 8..<9])
        #expect(ids("fit") == ["G"], "fixInvoiceTotals")
        #expect(ids("setup") == ["B"], "a word start")
    }

    @Test func pinyinFindsChineseNames() {
        #expect(ids("jkps") == ["C"], "initials")
        #expect(ids("jiekou") == ["C"], "full pinyin from the start")
        #expect(index.search("jiekou").first?.nameRanges == [0..<2])
        #expect(ids("jiek") == ["C"], "a partial syllable")
        #expect(ids("接口") == ["C"])
    }

    @Test func everyWordHasToMatch() {
        #expect(ids("api shop") == ["A"], "C is in shop too but has no api")
        #expect(ids("zzz").isEmpty)
        #expect(ids("   ").isEmpty)
    }

    @Test func searchingIsOneRankedListThenTheClosedOnes() {
        let live = index.search("api")
        let items = SessionList.searchItems(query: "api", live: live, closed: Array(live.prefix(1)))
        #expect(items.first == .askMoss("api"), "not what you need? ask Moss, above the results")
        #expect(items[1] == .hit(live[0], closed: false))
        #expect(items.contains(.closedHeader))
        #expect(!items.contains(.pinnedHeader), "no Pinned while searching")
        #expect(Set(items.map(\.id)).count == items.count)
        #expect(SessionList.searchItems(query: " zzz ", live: [], closed: []) == [.noMatch("zzz")])
    }

    @Test func wordsAreSplitWhereTheNameBreaks() {
        let words = SearchIndex.words("mac-dev_setup Now").map(\.text)
        #expect(words == ["mac", "dev", "setup", "now"])
        #expect(SearchIndex.words("fixInvoiceTotals").map(\.text) == ["fix", "invoice", "totals"])
        #expect(SearchIndex.words("接口").map(\.text) == ["jie", "kou"])
    }
}
