import Foundation
import Testing
@testable import MossKit

@Suite struct MentionFlowTests {
    private func split(_ text: String) -> [String] {
        let chars = Array(text)
        return MentionFlow.units(text).map { String(chars[$0]) }
    }

    @Test func latinWrapsByWordWithItsSpace() {
        #expect(split("is still working, leave it") == ["is ", "still ", "working, ", "leave ", "it"])
    }

    @Test func chineseWrapsByCharacter() {
        #expect(split("卡在权限") == ["卡", "在", "权", "限"])
    }

    @Test func closingPunctuationStaysWithWhatsBefore() {
        // Full-width punctuation never ends a unit mid-text (it would be set
        // half-width): the next character comes along.
        #expect(split("批完了。好") == ["批", "完", "了。好"])
        #expect(split("权限，等你") == ["权", "限，等", "你"])
        #expect(split("（括号）里") == ["（括", "号）里"])
        #expect(split("好了。") == ["好", "了。"], "at the end it stays")
        #expect(split("意见，5 条") == ["意", "见，5 ", "条"], "a word after it comes along whole")
    }

    @Test func openingPunctuationGoesWithWhatFollows() {
        #expect(split("说「好的」") == ["说", "「好", "的」"])
        #expect(split("权限，「好」") == ["权", "限，", "「好」"], "an opener after a comma starts its own unit")
        #expect(split("see (the logs)") == ["see ", "(the ", "logs)"])
    }

    @Test func mixedLatinAndChinese() {
        #expect(split("把 0.2 的说明") == ["把 ", "0.2 ", "的", "说", "明"])
        #expect(split("跑 tests 了") == ["跑 ", "tests ", "了"])
    }

    @Test func mentionsArePiecesOfTheirOwnAndStylesAreKept() {
        let names: [MossMentions.Name] = [.init("接口评审", id: "01B")]
        let marked = MossMentions.mark(MossMarkdown.inline("**接口评审** 在等你 `ok`"), names: names)
        let pieces = MentionFlow.pieces(marked)
        #expect(pieces.first == .mention(id: "01B", name: "接口评审"))
        let texts = pieces.compactMap { piece -> String? in
            if case let .text(t) = piece { return String(t.characters) } else { return nil }
        }
        #expect(texts == ["在", "等", "你 ", "ok"], "the space between the badge and Chinese text goes")
        if case let .text(code)? = pieces.last {
            #expect(code.runs.first?.inlinePresentationIntent?.contains(.code) == true, "the code span stays code")
        }
    }

    @Test func theBadgeKeepsItsOwnRoomSoTheSpacesAroundItGo() {
        let marked = MossMentions.mark(MossMarkdown.inline("Both **api-review** and more"), names: [.init("api-review", id: "A")])
        let texts = MentionFlow.pieces(marked).compactMap { piece -> String? in
            if case let .text(t) = piece { return String(t.characters) } else { return nil }
        }
        #expect(texts == ["Both", "and ", "more"])
    }

    @Test func punctuationAfterABadgeWrapsWithIt() {
        let marked = MossMentions.mark(MossMarkdown.inline("最忙的是登录页，它在跑"), names: [.init("登录页", id: "A")])
        let pieces = MentionFlow.pieces(marked)
        let glued = MentionFlow.glued(pieces)
        let i = pieces.firstIndex(of: .mention(id: "A", name: "登录页"))!
        #expect(glued[i + 1], "the comma can't start a line")
        #expect(!glued[i], "the badge itself can")
    }

    @Test func aLineBreakInTheParagraphIsKept() {
        let marked = MossMentions.mark(MossMarkdown.inline("one\ntwo"), names: [])
        #expect(MentionFlow.pieces(marked).count == 3)
        #expect(MentionFlow.pieces(marked)[1] == .lineBreak)
    }
}
