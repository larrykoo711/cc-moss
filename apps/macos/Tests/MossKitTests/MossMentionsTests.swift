import Foundation
import Testing
@testable import MossKit

@Suite struct MossMentionsTests {
    private func found(_ text: String, _ names: [MossMentions.Name]) -> [String] {
        let chars = Array(text)
        return MossMentions.find(text: text, names: names).map { String(chars[$0.range]) + "=" + $0.id }
    }

    @Test func aChineseNameIsFoundInsideChineseWords() {
        #expect(found("接口评审在等你批一个权限。", [.init("接口评审", id: "01B")]) == ["接口评审=01B"])
    }

    @Test func aChineseNameGluedToLatinLettersIsNot() {
        #expect(found("x接口评审", [.init("接口评审", id: "01B")]).isEmpty)
    }

    @Test func latinNamesAreWholeWords() {
        let names: [MossMentions.Name] = [.init("api-review", id: "A")]
        #expect(found("api-review is waiting.", names) == ["api-review=A"])
        #expect(found("(api-review), ok", names) == ["api-review=A"], "punctuation bounds it")
        #expect(found("my-api-review", names).isEmpty)
        #expect(found("api-reviewer", names).isEmpty)
    }

    @Test func theLongestNameWinsAndNothingOverlaps() {
        let names: [MossMentions.Name] = [.init("api", id: "S"), .init("api review", id: "L")]
        #expect(found("see api review and api", names) == ["api review=L", "api=S"])
    }

    @Test func theFirstSessionWithANameIsTheOne() {
        #expect(found("job-a ran", [.init("job-a", id: "mine"), .init("job-a", id: "other")]) == ["job-a=mine"])
    }

    @Test func codeLinksAndURLsAreLeftAlone() {
        let names: [MossMentions.Name] = [.init("api-review", id: "A")]
        #expect(found("see https://example.com/api-review now", names).isEmpty)
        let marked = MossMentions.mark(MossMarkdown.inline("`api-review` and api-review"), names: names)
        let links = marked.runs.compactMap { run in run.link.map { _ in String(marked[run.range].characters) } }
        #expect(links == ["api-review"], "the one outside the code span")
    }

    @Test func boldNamesBecomeMentions() {
        let marked = MossMentions.mark(MossMarkdown.inline("**接口评审** 在等你"), names: [.init("接口评审", id: "01B")])
        let url = marked.runs.compactMap(\.link).first
        #expect(url.flatMap(MossMentions.id(of:)) == "01B")
    }

    @Test func shortNamesAreIgnored() {
        #expect(found("go to ab now", [.init("ab", id: "x")]).isEmpty)
        #expect(found("改", [.init("改", id: "x")]).isEmpty)
    }

    @Test func otherLinksAreNotMentions() {
        #expect(MossMentions.id(of: URL(string: "https://example.com/x")!) == nil)
        #expect(MossMentions.url("01B").flatMap(MossMentions.id(of:)) == "01B")
    }
}
