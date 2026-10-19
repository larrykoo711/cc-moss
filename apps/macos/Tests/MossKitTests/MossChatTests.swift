import Foundation
import Testing
@testable import MossKit

private let chatJSON = #"""
{"schema":1,"messages":[
 {"id":"1760608800000-a1b2c3","at":"2026-10-16T10:00:00Z","from":"events","text":"[动态] api-review 在等你","fallback":false,"buttons":[]},
 {"id":"1760608810000-a1b2c4","at":"2026-10-16T10:00:10Z","from":"you","text":"谁在等我？","state":"replied","fallback":false,"buttons":[]},
 {"id":"1760608830000-a1b2c5","at":"2026-10-16T10:00:30Z","from":"moss","text":"**api-review** 在等你点头。","reply_to":"1760608810000-a1b2c4","fallback":false,"buttons":["去看看","先不管"]},
 {"id":"1760608840000-a1b2c6","at":"2026-10-16T10:00:40Z","from":"you","text":"再说一次","state":"failed","error":"Moss didn't take it","fallback":false,"buttons":[]},
 {"id":"1760608850000-a1b2c7","at":"2026-10-16T10:00:50Z","from":"moss","text":"","reply_to":"x","fallback":true,"buttons":[]}
]}
"""#

@Suite struct MossChatTests {
    @Test func decodesEverySender() throws {
        let chat = try MossChat.decode(Data(chatJSON.utf8))
        #expect(chat.messages.map(\.from) == [.events, .you, .moss, .you, .moss])
        #expect(chat.messages[0].at == parseTimestamp("2026-10-16T10:00:00Z"))
    }

    @Test func yourMessagesCarryTheirState() throws {
        let chat = try MossChat.decode(Data(chatJSON.utf8))
        #expect(chat.messages[1].delivery == .replied)
        #expect(chat.messages[3].delivery == .failed)
        #expect(chat.messages[3].error == "Moss didn't take it")
        #expect(chat.messages[2].delivery == nil)
    }

    @Test func buttonsAndReplyToAreRead() throws {
        let reply = try MossChat.decode(Data(chatJSON.utf8)).messages[2]
        #expect(reply.buttons == ["去看看", "先不管"])
        #expect(reply.replyTo == "1760608810000-a1b2c4")
    }

    @Test func aFallbackReadsAsAReceipt() throws {
        let m = try MossChat.decode(Data(chatJSON.utf8)).messages[4]
        #expect(m.fallback)
        #expect(m.shownText(.zh) == "Moss 收到了（没能取回回复，去终端看）")
    }

    @Test func missingFieldsHaveDefaults() throws {
        let chat = try MossChat.decode(Data(#"{"schema":1,"messages":[{"id":"1","from":"moss","text":"hi"}]}"#.utf8))
        let m = chat.messages[0]
        #expect(m.at == nil && !m.fallback && m.buttons.isEmpty && m.replyTo == nil && m.delivery == nil)
    }

    @Test func noMessagesIsAnEmptyChat() throws {
        #expect(try MossChat.decode(Data(#"{"schema":1}"#.utf8)).messages.isEmpty)
        #expect(try MossChat.decode(Data(#"{"schema":1,"messages":[]}"#.utf8)).messages.isEmpty)
    }

    @Test func unknownSenderAndStateAreKept() throws {
        let m = try MossChat.decode(Data(#"{"schema":1,"messages":[{"id":"1","from":"slack","text":"hi","state":"later"}]}"#.utf8)).messages[0]
        #expect(m.from == .other("slack"))
        #expect(m.delivery == nil)
    }

    @Test func blankButtonsAreDropped() throws {
        let m = try MossChat.decode(Data(#"{"schema":1,"messages":[{"id":"1","from":"moss","text":"a","buttons":["好"," ",""]}]}"#.utf8)).messages[0]
        #expect(m.buttons == ["好"])
    }

    @Test func newerSchemaIsRefused() {
        #expect(throws: MossClient.Failure.newerSchema(2)) {
            try MossChat.decode(Data(#"{"schema":2,"messages":[]}"#.utf8))
        }
    }

    @Test func chatAndSayGoThroughTheCLI() async throws {
        let runner = AskRunner([(["moss", "chat"], ok(chatJSON)),
                                (["moss", "say"], ok(#"{"schema":1,"id":"1760608900000-ffffff"}"#))])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        #expect(try await client.mossChat().messages.count == 5)
        #expect(try await client.mossSay("-n hello") == "1760608900000-ffffff")
        #expect(runner.calls == [["moss", "chat", "--json"], ["moss", "say", "--json", "--", "-n hello"]])
    }

    @Test func aRefusedSayIsCCMossOwnWords() async {
        let runner = AskRunner([(["moss", "say"], fail("cc-moss: Moss is off.\n"))])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        await #expect(throws: MossClient.Failure.command("cc-moss: Moss is off.")) {
            _ = try await client.mossSay("hi")
        }
    }

    @MainActor @Test func sendingShowsAtOnceThenQueues() async {
        var said: [String] = []
        let desk = MossDesk { text in said.append(text); return "1760608900000-000001" }
        await desk.send("  hello  ")
        #expect(said == ["hello"])
        #expect(desk.messages.map(\.id) == ["1760608900000-000001"])
        #expect(desk.messages[0].delivery == .queued)
    }

    @MainActor @Test func aLoggedMessageReplacesTheLocalOne() async throws {
        let desk = MossDesk { _ in "1760608810000-a1b2c4" }
        desk.update(chat: MossChat())
        await desk.send("谁在等我？")
        desk.update(chat: try MossChat.decode(Data(chatJSON.utf8)))
        #expect(desk.messages.count == 4, "the digest isn't shown")
        #expect(desk.pending.isEmpty)
    }

    @MainActor @Test func aRefusedSendIsFailedWithTheReason() async {
        let desk = MossDesk { _ in throw MossClient.Failure.command("Moss is off.") }
        await desk.send("hi")
        #expect(desk.messages.first?.delivery == .failed)
        #expect(desk.messages.first?.error == "Moss is off.")
    }

    @MainActor @Test func resendSendsTheSameWordsAgain() async throws {
        var said: [String] = []
        let desk = MossDesk { text in said.append(text); return "1760608990000-000002" }
        desk.update(chat: try MossChat.decode(Data(chatJSON.utf8)))
        await desk.resend("1760608840000-a1b2c6")
        #expect(said == ["再说一次"])
        #expect(desk.resent.contains("1760608840000-a1b2c6"))
    }

    @MainActor @Test func aLocalFailureIsReplacedOnResend() async {
        var fail = true
        let desk = MossDesk { _ in
            if fail { fail = false; throw MossClient.Failure.timedOut }
            return "1760608990000-000003"
        }
        await desk.send("hi")
        let failed = desk.messages[0].id
        await desk.resend(failed)
        #expect(desk.messages.map(\.id) == ["1760608990000-000003"])
    }

    @Test func onlyHttpsLinksStayLinks() {
        let text = MossMarkdown.inline("[docs](https://example.com) [file](file:///etc/passwd) [x](javascript:alert(1))")
        let links = text.runs.compactMap(\.link)
        #expect(links == [URL(string: "https://example.com")!])
        #expect(String(text.characters) == "docs file x")
    }

    @Test func lineBreaksAreNormalised() {
        #expect(MossMarkdown.normalised("a\r\nb\rc\nd") == "a\nb\nc\nd")
    }

    @MainActor @Test func carriageReturnsNeverReachTheCLI() async {
        var said: [String] = []
        let desk = MossDesk { text in said.append(text); return "1760608900000-000009" }
        await desk.send("one\r\ntwo\rthree")
        #expect(said == ["one\ntwo\nthree"])
    }

    @MainActor @Test func aSuggestedReplyIsSentOncePerMessage() async {
        var said: [String] = []
        let desk = MossDesk { text in said.append(text); return "1760608900000-00000\(said.count)" }
        await desk.choose("去看看", on: "m1")
        await desk.choose("先不管", on: "m1")
        #expect(said == ["去看看"])
        #expect(desk.answered.contains("m1"))
    }

    @MainActor @Test func unreadUntilThePageIsSeen() throws {
        let desk = MossDesk { _ in "" }
        let chat = try MossChat.decode(Data(chatJSON.utf8))
        desk.update(chat: MossChat(messages: Array(chat.messages.prefix(2))))
        #expect(!desk.unread, "history from before launch is read")
        desk.update(chat: chat)
        #expect(desk.unread)
        desk.markRead()
        #expect(!desk.unread)
    }
}
