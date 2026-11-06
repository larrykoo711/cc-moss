import Foundation
import Testing
@testable import MossKit

@Suite struct SaidSearchTests {
    private func session(_ id: String, name: String, moss: Bool = false) -> Session {
        var s = sessions([sessionJSON(id: id, name: name)])[0]
        if moss { s.role = "moss" }
        return s
    }

    private let found = #"""
    {"schema":1,"query":"退款","partial":false,"indexing":true,"matches":[
      {"id":"B","name":"b","status":"closed","archived":true,"session_id":"s-b","at":"2026-11-05T08:00:00Z","speaker":"you","snippet":"把退款逻辑改成按订单分摊"},
      {"id":"A","name":"a","status":"running","archived":false,"session_id":"s-a","at":"2026-11-04T08:00:00Z","speaker":"reply","snippet":"refunds split"},
      {"id":"M","name":"Moss","status":"running","archived":false,"session_id":"s-m","at":"2026-11-03T08:00:00Z","speaker":"you","snippet":"x"},
      {"id":"Z","name":"gone","status":"closed","archived":false,"session_id":"s-z","at":"2026-11-02T08:00:00Z","speaker":"you","snippet":"y"}]}
    """#

    @Test func decodesWhatFindSays() throws {
        let r = try SaidResult.decode(Data(found.utf8))
        #expect(r.matches.map(\.id) == ["B", "A", "M", "Z"])
        #expect(r.indexing && !r.partial)
        #expect(r.matches[0].snippet == "把退款逻辑改成按订单分摊")
        #expect(r.matches[0].at != nil)
        #expect(throws: (any Error).self) { try SaidResult.decode(Data(#"{"schema":2,"matches":[]}"#.utf8)) }
    }

    @Test func hitsKeepTheOrderAndLeaveOutWhatShowsAlready() throws {
        let r = try SaidResult.decode(Data(found.utf8))
        let pool = [session("A", name: "a"), session("B", name: "b"), session("M", name: "Moss", moss: true)]
        let hits = SaidSearch.hits(r, pool: pool, shown: ["A"])
        // A is listed already, Moss is never a result, Z isn't known here.
        #expect(hits.map(\.session.id) == ["B"])
        #expect(hits[0].reason == .said("把退款逻辑改成按订单分摊"))
        #expect(SaidSearch.hits(r, pool: pool, shown: []).map(\.session.id) == ["B", "A"])
    }

    @Test func onlyQueriesWorthAsking() {
        #expect(!SaidSearch.asks(" 退 "))
        #expect(SaidSearch.asks("退款"))
    }

    @Test func saidHitsAloneAreResultsNotNoMatch() throws {
        let r = try SaidResult.decode(Data(found.utf8))
        let said = SaidSearch.hits(r, pool: [session("B", name: "b")], shown: [])
        let items = SessionList.searchItems(query: "退款", live: [], closed: [], said: said)
        #expect(items.map(\.id) == ["ask-moss", "header:said", "said:B"])
        #expect(SessionList.searchItems(query: "退款", live: [], closed: [], said: []) == [.noMatch("退款")])
    }

    @Test func mossMaySearchOnlyWhenTheConfigSaysTrue() {
        #expect(MossSearch.isOn("true"))
        #expect(MossSearch.isOn(" TRUE\n"))
        #expect(!MossSearch.isOn("false"))
        #expect(!MossSearch.isOn(nil))
        #expect(!MossSearch.isOn(""))
    }
}
