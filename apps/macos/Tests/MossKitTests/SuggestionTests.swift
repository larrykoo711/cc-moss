import Foundation
import Testing
@testable import MossKit

@Suite struct SuggestionTests {
    @Test func anUntouchedEmptyFieldTakesTheSuggestion() {
        #expect(Suggestion.prefill(text: "", touched: false, suggestion: " open the about page ") == "open the about page")
    }

    @Test func typedTextIsNeverOverwritten() {
        #expect(Suggestion.prefill(text: "fix the", touched: true, suggestion: "run the tests") == nil)
        #expect(Suggestion.prefill(text: "", touched: true, suggestion: "run the tests") == nil, "typed and cleared again")
        #expect(Suggestion.prefill(text: "x", touched: false, suggestion: "run the tests") == nil)
    }

    @Test func noSuggestionLeavesTheField() {
        #expect(Suggestion.prefill(text: "", touched: false, suggestion: nil) == nil)
        #expect(Suggestion.prefill(text: "", touched: false, suggestion: "  ") == nil)
    }

    @Test func decodesTheCLIAnswer() {
        #expect(Suggestion.decode(Data(#"{"schema":1,"suggestion":"在浏览器打开 about 页看下"}"#.utf8)) == "在浏览器打开 about 页看下")
        #expect(Suggestion.decode(Data(#"{"schema":1,"suggestion":null}"#.utf8)) == nil)
        #expect(Suggestion.decode(Data("garbage".utf8)) == nil)
        #expect(Suggestion.decode(Data(#"{"schema":2,"suggestion":"x"}"#.utf8)) == nil)
    }

    @Test func anOlderCLIIsNoSuggestion() async {
        let client = MossClient(executable: "/x/cc-moss", runner: AskRunner([(["suggest"], fail("error: unrecognized subcommand 'suggest'"))]))
        #expect(await client.suggest("api-review") == nil)
        let newer = AskRunner([(["suggest"], ok(#"{"schema":1,"suggestion":"run it"}"#))])
        #expect(await MossClient(executable: "/x/cc-moss", runner: newer).suggest("api-review") == "run it")
        #expect(newer.calls == [["suggest", "api-review", "--json"]])
    }
}

@Suite struct ReplyLineTests {
    let list = sessions([
        sessionJSON(id: "A", name: "api", attention: "input"),
        sessionJSON(id: "B", name: "web", busy: true),
        sessionJSON(id: "C", name: "docs"),
        sessionJSON(id: "D", name: "ask", attention: "permission"),
        sessionJSON(id: "E", name: "old", status: "interrupted"),
        sessionJSON(id: "F", name: "out", tmux: false, attention: "input"),
        sessionJSON(id: "G", name: "ui", attention: "input"),
    ])

    private func s(_ id: String) -> Session { list.first { $0.id == id }! }

    @Test func whichRowsOpenALine() {
        #expect(ReplyLine.opens(s("A")) && ReplyLine.opens(s("C")) && ReplyLine.opens(s("G")))
        #expect(!ReplyLine.opens(s("B")), "busy")
        #expect(!ReplyLine.opens(s("D")), "asking for a permission: answered on To Do")
        #expect(!ReplyLine.opens(s("E")), "dropped")
        #expect(!ReplyLine.opens(s("F")), "not in tmux")
    }

    @Test func mossNeverOpensALine() {
        var moss = s("A")
        moss.role = "moss"
        #expect(!ReplyLine.opens(moss))
    }

    @Test func returnSendsTheFieldsText() {
        #expect(ReplyLine.toSend("run the tests") == "run the tests")
        #expect(ReplyLine.toSend("   ") == nil)
        #expect(ReplyLine.toSend("") == nil)
    }

    @Test func theNextWaitingSessionInListOrder() {
        #expect(ReplyLine.next(after: "A", in: list)?.id == "G")
        #expect(ReplyLine.next(after: "G", in: list)?.id == "A", "wraps to the top")
        #expect(ReplyLine.next(after: "C", in: list)?.id == "G")
        #expect(ReplyLine.next(after: "A", in: list, backwards: true)?.id == "G")
    }

    @Test func noOtherWaitingMeansNone() {
        let one = [s("A"), s("C")]
        #expect(ReplyLine.next(after: "A", in: one) == nil)
    }
}
