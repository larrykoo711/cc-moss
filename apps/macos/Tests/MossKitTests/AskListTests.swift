import Foundation
import Testing
@testable import MossKit

@Suite struct AskListTests {
    @Test func decodesAQuestion() throws {
        let list = try AskList.decode(askListData([askJSON()]))
        #expect(list.count == 1)
        let a = list[0]
        #expect(a.id == "a1b2c3d4e5f6a7b8")
        #expect(a.kind == .question)
        #expect(a.sessionName == "job-a")
        #expect(a.questions == [Ask.Question(text: "Which database?", header: "Database",
                                             options: [Ask.Option(label: "Postgres", description: "about Postgres"),
                                                       Ask.Option(label: "SQLite", description: "about SQLite")])])
        #expect(a.expiresAt.timeIntervalSince(a.createdAt) == 300)
    }

    @Test func decodesSeveralQuestionsAndMultipleChoice() {
        let a = ask(askJSON(questions: [questionJSON("First?"), questionJSON("Pick any?", labels: ["x", "y", "z"], multi: true)]))
        #expect(a.questions.count == 2)
        #expect(a.questions[1].multiSelect)
        #expect(a.questions[1].options.map(\.label) == ["x", "y", "z"])
    }

    @Test func decodesAPermission() {
        let a = ask(askJSON(kind: "permission", name: nil))
        #expect(a.kind == .permission)
        #expect(a.tool == Ask.Tool(name: "Bash", preview: "rm -rf build/"))
        #expect(a.sessionName == nil)
        #expect(a.questions.isEmpty)
    }

    @Test func skipsAnAskWithFieldsMissing() throws {
        var noId = askJSON(id: "x"); noId.removeValue(forKey: "id")
        var noQuestions = askJSON(id: "y"); noQuestions.removeValue(forKey: "questions")
        var noTool = askJSON(id: "z", kind: "permission"); noTool.removeValue(forKey: "tool")
        let badOption = askJSON(id: "w", questions: [["question": "Q?", "options": [["description": "no label"]]]])
        let unknownKind = askJSON(id: "v", kind: "elicitation")
        let good = askJSON(id: "good")
        let list = try AskList.decode(askListData([noId, noQuestions, noTool, badOption, unknownKind, good]))
        #expect(list.map(\.id) == ["good"])
    }

    @Test func refusesANewerSchema() {
        #expect(throws: AskListError.newerSchema(2)) { try AskList.decode(askListData([], schema: 2)) }
    }

    @Test func refusesSomethingElse() {
        #expect(throws: AskListError.self) { try AskList.decode(Data("[]".utf8)) }
        #expect(throws: AskListError.self) { try AskList.decode(Data("{}".utf8)) }
    }

    @Test func emptyListIsEmpty() throws {
        #expect(try AskList.decode(Data(#"{"schema":1,"asks":[]}"#.utf8)).isEmpty)
    }

    @Test func clientRunsAskList() async throws {
        let runner = AskRunner([(["ask", "list"], ok(String(decoding: askListData([askJSON()]), as: UTF8.self)))])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        let list = try await client.asks()
        #expect(list.count == 1)
        #expect(runner.calls == [["ask", "list", "--json"]])
    }
}
