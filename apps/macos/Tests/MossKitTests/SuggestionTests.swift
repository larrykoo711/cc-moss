import Foundation
import Testing
@testable import MossKit

@Suite struct SuggestionTests {
    let usual = "What should Claude do next?"

    @Test func anEmptyFieldShowsTheSuggestion() {
        let p = Suggestion.placeholder(text: "", suggestion: "open the about page in a browser", fallback: usual)
        #expect(p.text == "open the about page in a browser" && p.isSuggestion)
    }

    @Test func noSuggestionMeansTheUsualPlaceholder() {
        #expect(Suggestion.placeholder(text: "", suggestion: nil, fallback: usual) == (usual, false))
        #expect(Suggestion.placeholder(text: "", suggestion: "  ", fallback: usual) == (usual, false))
    }

    @Test func tabFillsOnlyWhenEmpty() {
        #expect(Suggestion.fill(text: "", suggestion: " run the tests ") == "run the tests")
        #expect(Suggestion.fill(text: "", suggestion: nil) == nil)
    }

    @Test func typedTextWins() {
        #expect(Suggestion.fill(text: "fix the", suggestion: "run the tests") == nil)
        #expect(!Suggestion.placeholder(text: "fix the", suggestion: "run the tests", fallback: usual).isSuggestion)
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
