import Foundation
import Testing
@testable import MossKit

@Suite struct AskCardTests {
    private let id = "a1b2c3d4e5f6a7b8"

    // MARK: - What goes to `ask answer`

    @Test func singleChoiceArguments() {
        #expect(AskReply.choices([["SQLite"]]).arguments(id: id)
            == ["ask", "answer", id, "--choice", "SQLite", "--by", "desktop", "--json"])
    }

    @Test func severalQuestionsAreNumbered() {
        #expect(AskReply.choices([["a"], ["x", "z"]]).arguments(id: id)
            == ["ask", "answer", id, "--choice", "1:a", "--choice", "2:x", "--choice", "2:z", "--by", "desktop", "--json"])
    }

    @Test func aLabelThatLooksNumberedIsNumbered() {
        // "1:00 PM" alone would read as "question 1, option 00 PM".
        #expect(AskReply.choices([["1:00 PM"]]).arguments(id: id)
            == ["ask", "answer", id, "--choice", "1:1:00 PM", "--by", "desktop", "--json"])
    }

    @Test func textAllowDenyArguments() {
        #expect(AskReply.text("use MySQL").arguments(id: id) == ["ask", "answer", id, "--text", "use MySQL", "--by", "desktop", "--json"])
        #expect(AskReply.allow.arguments(id: id) == ["ask", "answer", id, "--allow", "--by", "desktop", "--json"])
        #expect(AskReply.deny.arguments(id: id) == ["ask", "answer", id, "--deny", "--by", "desktop", "--json"])
    }

    // MARK: - Choosing

    @Test func singleChoiceAnswersOnClick() {
        var card = AskCard(ask())
        #expect(card.choose("SQLite") == .choices([["SQLite"]]))
        #expect(card.phase == .submitting)
    }

    @Test func aLabelNotOfferedDoesNothing() {
        var card = AskCard(ask())
        #expect(card.choose("MySQL") == nil)
        #expect(card.phase == .choosing)
    }

    @Test func noSecondSendWhileSubmitting() {
        var card = AskCard(ask())
        _ = card.choose("SQLite")
        #expect(card.choose("Postgres") == nil)
        #expect(card.sendText() == nil)
        #expect(card.sent == .choices([["SQLite"]]))
    }

    @Test func multipleChoiceWaitsForDone() {
        var card = AskCard(ask(askJSON(questions: [questionJSON("Pick any?", labels: ["x", "y", "z"], multi: true)])))
        #expect(card.confirm() == nil) // nothing picked yet
        #expect(card.choose("z") == nil)
        #expect(card.choose("x") == nil)
        #expect(card.choose("y") == nil)
        #expect(card.choose("y") == nil) // off again
        #expect(card.picked("x") && card.picked("z") && !card.picked("y"))
        #expect(card.confirm() == .choices([["x", "z"]])) // the session's order
    }

    @Test func severalQuestionsOneByOneThenSend() {
        var card = AskCard(ask(askJSON(questions: [questionJSON("First?", labels: ["a", "b"]),
                                                   questionJSON("Second?", labels: ["x", "y"], multi: true)])))
        #expect(card.question?.text == "First?")
        #expect(card.choose("b") == nil)
        #expect(card.question?.text == "Second?")
        #expect(card.choose("y") == nil)
        #expect(card.confirm() == nil)
        #expect(card.reviewing)
        card.back()
        #expect(card.question?.text == "Second?")
        #expect(card.picked("y"))
        #expect(card.confirm() == nil)
        #expect(card.sendAll() == .choices([["b"], ["y"]]))
        #expect(card.phase == .submitting)
    }

    @Test func changeOneAnswerFromTheReviewAndComeStraightBack() {
        var card = AskCard(ask(askJSON(questions: [questionJSON("First?", labels: ["a", "b"]),
                                                   questionJSON("Second?", labels: ["x", "y"]),
                                                   questionJSON("Third?", labels: ["p", "q"])])))
        _ = card.choose("a"); _ = card.choose("x"); _ = card.choose("p")
        #expect(card.reviewing)
        card.revisit(0)
        #expect(card.question?.text == "First?")
        #expect(card.picked("a"))
        #expect(card.choose("b") == nil)
        #expect(card.reviewing, "back on the review, not on the second question")
        #expect(card.sendAll() == .choices([["b"], ["x"], ["p"]]))
    }

    @Test func revisitOnlyFromTheReview() {
        var card = AskCard(ask(askJSON(questions: [questionJSON("First?"), questionJSON("Second?")])))
        card.revisit(1)
        #expect(card.question?.text == "First?")
    }

    @Test func writeYourOwnForASingleQuestion() {
        var card = AskCard(ask())
        #expect(card.canWrite)
        card.writing = true
        card.draft = "   "
        #expect(card.sendText() == nil)
        card.draft = " MySQL, please \n"
        #expect(card.sendText() == .text("MySQL, please"))
    }

    @Test func typeSomethingIsTheLastRowOfASingleQuestion() {
        var card = AskCard(ask())
        #expect(card.offersOwnWords)
        card.startWriting()
        #expect(card.writing)
        card.draft = "Use Postgres"
        card.stopWriting()
        #expect(!card.writing, "Esc goes back to the answers")
        #expect(card.draft == "Use Postgres", "and keeps what was typed")
        card.startWriting()
        #expect(card.sendText() == .text("Use Postgres"))
        #expect(!card.offersOwnWords, "on its way: no more typing")
    }

    @Test func noTypeSomethingRowForPermissionsOrTheReview() {
        #expect(!AskCard(ask(askJSON(kind: "permission"))).offersOwnWords)
        var card = AskCard(ask(askJSON(questions: [questionJSON("First?"), questionJSON("Second?")])))
        #expect(card.offersOwnWords, "every question step has it")
        _ = card.choose("Postgres")
        _ = card.choose("SQLite")
        #expect(card.reviewing)
        #expect(!card.offersOwnWords, "not on the review")
    }

    private func two() -> AskCard {
        AskCard(ask(askJSON(questions: [questionJSON("First?"), questionJSON("Second?", labels: ["yes", "no"])])))
    }

    @Test func ownWordsAnswerOneQuestionOfSeveral() {
        var card = two()
        _ = card.choose("SQLite")
        card.startWriting()
        card.draft = " only on Fridays \n"
        #expect(card.sendText() == nil, "with several, it answers this one and moves on")
        #expect(card.reviewing)
        #expect(card.typed(1) == "only on Fridays")
        let reply = card.sendAll()
        #expect(reply == .answers([.picked(["SQLite"]), .typed("only on Fridays")]))
    }

    @Test func eachQuestionKeepsItsOwnDraft() {
        var card = two()
        card.startWriting()
        card.draft = "first thoughts"
        card.stopWriting()
        _ = card.choose("Postgres")
        card.startWriting()
        card.draft = "second thoughts"
        card.back()
        #expect(card.draft == "first thoughts")
    }

    @Test func pickingAnOptionReplacesTypedWords() {
        var card = two()
        card.startWriting()
        card.draft = "my own"
        _ = card.sendText()
        card.back()
        #expect(card.writing, "back on it: its words are open again")
        _ = card.choose("Postgres")
        #expect(card.typed(0) == nil)
        _ = card.choose("no")
        #expect(card.sendAll() == .choices([["Postgres"], ["no"]]))
    }

    @Test func mixedAnswersAreSentNumbered() {
        let reply = AskReply.answers([.picked(["a", "b"]), .typed("1: not a choice")])
        #expect(reply.arguments(id: id) == ["ask", "answer", id, "--choice", "1:a", "--choice", "1:b",
                                            "--text", "2:1: not a choice", "--by", "desktop", "--json"])
    }

    @Test func emptyWordsDoNotAnswer() {
        var card = two()
        card.startWriting()
        card.draft = "   "
        #expect(card.sendText() == nil)
        #expect(card.step == 0, "still on the first question")
    }

    @Test func permissionAllowAndDeny() {
        var card = AskCard(ask(askJSON(kind: "permission")))
        #expect(card.choose("Postgres") == nil)
        #expect(card.allow() == .allow)
        var other = AskCard(ask(askJSON(kind: "permission")))
        #expect(other.deny() == .deny)
        var question = AskCard(ask())
        #expect(question.allow() == nil)
    }

    // MARK: - How it went

    private func result(_ r: String, status: Int32 = 0) -> CommandResult {
        CommandResult(status: status, stdout: Data(#"{"schema":1,"result":"\#(r)","session":"job-a"}"#.utf8) + Data("\n".utf8),
                      stderr: status == 0 ? "" : "cc-moss: too late: job-a already moved on\n")
    }

    @Test func answeredFromJSON() {
        #expect(AskOutcome.from(result("answered")) == .answered)
    }

    @Test func notWaitingFromJSON() {
        #expect(AskOutcome.from(result("not-waiting")) == .notWaiting)
    }

    @Test func tooLateFromJSON() {
        // Too late exits non-zero; the line still says what happened.
        #expect(AskOutcome.from(result("too-late", status: 1)) == .late)
    }

    @Test func anUnknownResultIsAFailure() {
        guard case .failed = AskOutcome.from(result("maybe")) else {
            Issue.record("an unknown result must not count as answered")
            return
        }
    }

    @Test func noLineIsCcMossOwnWords() {
        #expect(AskOutcome.from(fail("cc-moss: the session could not use that answer")) == .failed("the session could not use that answer"))
        // The words are not read any more: only the JSON line counts.
        guard case .failed = AskOutcome.from(ok("Answered. job-a carries on with it.")) else {
            Issue.record("text without --json must not count as answered")
            return
        }
    }

    @Test func aRefusalLeavesTheCardAnswerable() {
        var card = AskCard(ask())
        _ = card.choose("SQLite")
        card.finish(.failed("the session could not use that answer"))
        #expect(card.phase == .choosing)
        #expect(card.problem == "the session could not use that answer")
        #expect(card.choose("Postgres") == .choices([["Postgres"]]))
        #expect(card.problem == nil)
    }

    @Test func otherOutcomesCloseTheCard() {
        for outcome in [AskOutcome.answered, .late, .notWaiting] {
            var card = AskCard(ask())
            _ = card.choose("SQLite")
            card.finish(outcome)
            #expect(card.phase == .finished(outcome))
            #expect(card.choose("Postgres") == nil)
        }
    }

    @Test func clientAnswersWithTheRightArguments() async {
        let runner = AskRunner([(["ask", "answer"], result("answered"))])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        let outcome = await client.answer(id, with: .choices([["SQLite"]]))
        #expect(outcome == .answered)
        #expect(runner.calls == [["ask", "answer", id, "--choice", "SQLite", "--by", "desktop", "--json"]])
    }

    @Test func clientReportsLateAndNotWaiting() async {
        let late = MossClient(executable: "/x", runner: AskRunner([(["ask"], result("too-late", status: 1))]))
        #expect(await late.answer(id, with: .allow) == .late)
        let idle = MossClient(executable: "/x", runner: AskRunner([(["ask"], result("not-waiting"))]))
        #expect(await idle.answer(id, with: .allow) == .notWaiting)
    }
}
