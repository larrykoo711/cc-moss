import Foundation
import Testing
@testable import MossKit

@Suite struct MossQuickTests {
    let now = Date(timeIntervalSince1970: 1_760_608_800)
    private func ago(days: Double) -> Date { now.addingTimeInterval(-days * 86_400) }
    private func ids(_ items: [MossQuickItem]) -> [String] { items.map(\.id) }

    @Test func theItemsAreActionsThenCommandsForTheAccount() {
        #expect(ids(MossQuick.items(account: .subscription)) == [
            "action:needs-you", "action:stuck", "action:today",
            "cmd:/usage", "cmd:/status", "cmd:/context", "cmd:/stats",
        ])
        #expect(!ids(MossQuick.items(account: .api)).contains("cmd:/usage"))
        #expect(ids(MossQuick.items(account: .api)).contains("cmd:/cost"))
    }

    @Test func everyItemHasBothLanguagesAndUniqueIds() {
        let items = MossQuick.items()
        #expect(Set(ids(items)).count == items.count)
        for item in items {
            #expect(!item.title(.en).isEmpty && !item.title(.zh).isEmpty && !item.symbol.isEmpty, "\(item.id)")
        }
        let usage = items.first { $0.id == "cmd:/usage" }
        #expect(usage?.title(.zh) == "用量" && usage?.text(.zh) == "/usage")
    }

    @Test func clearAndCompactAreNotOffered() {
        let all = ids(MossQuick.items())
        #expect(!all.contains("cmd:/clear") && !all.contains("cmd:/compact"))
        #expect(MossQuick.item(sent: "/clear") == nil, "typed, it's still sent, just not counted")
    }

    @Test func moreListsOnlyWhatTheBarDoesntShowOnce() {
        let ranked = MossQuick.ranked(MossQuick.items(account: .subscription), uses: [], context: .init(), now: now)
        let more = MossQuick.overflow(ranked, shown: 3)
        #expect(ids(more) == ["cmd:/usage", "cmd:/status", "cmd:/context", "cmd:/stats"])
        #expect(Set(ids(more)).isDisjoint(with: ids(Array(ranked.prefix(3)))))
        #expect(MossQuick.overflow(ranked, shown: ranked.count).isEmpty)
    }

    @Test func withNoHistoryTheDefaultOrderStands() {
        let ranked = MossQuick.ranked(MossQuick.items(account: .subscription), uses: [], context: .init(), now: now)
        #expect(Array(ids(ranked).prefix(4)) == ["action:needs-you", "action:stuck", "action:today", "cmd:/usage"])
    }

    @Test func usedMostComesFirst() {
        let uses = (0..<3).map { _ in MossQuickUse(id: "cmd:/status", at: ago(days: 1)) }
            + [MossQuickUse(id: "action:today", at: ago(days: 0.5))]
        let ranked = MossQuick.ranked(MossQuick.items(), uses: uses, context: .init(), now: now)
        #expect(Array(ids(ranked).prefix(3)) == ["cmd:/status", "action:today", "action:needs-you"])
    }

    @Test func oldUsesFade() {
        #expect(abs(MossQuick.score("x", uses: [MossQuickUse(id: "x", at: ago(days: 7))], now: now) - 0.5) < 0.001)
        // Four uses a month ago weigh less than one yesterday.
        let old = (0..<4).map { _ in MossQuickUse(id: "cmd:/context", at: ago(days: 30)) }
        let new = [MossQuickUse(id: "cmd:/stats", at: ago(days: 1))]
        let ranked = MossQuick.ranked(MossQuick.items(), uses: old + new, context: .init(), now: now)
        #expect(ids(ranked).firstIndex(of: "cmd:/stats")! < ids(ranked).firstIndex(of: "cmd:/context")!)
    }

    @Test func theMomentBoosts() {
        let uses = (0..<2).map { _ in MossQuickUse(id: "cmd:/stats", at: now) }
        var ctx = MossQuick.Context(anyInterrupted: true)
        #expect(ids(MossQuick.ranked(MossQuick.items(), uses: uses, context: ctx, now: now)).first == "action:stuck")
        ctx = MossQuick.Context(usageHot: true)
        #expect(ids(MossQuick.ranked(MossQuick.items(account: .subscription), uses: uses, context: ctx, now: now)).first == "cmd:/usage")
        ctx = MossQuick.Context(queueHasItems: true)
        #expect(ids(MossQuick.ranked(MossQuick.items(), uses: uses, context: ctx, now: now)).first == "action:needs-you")
    }

    @Test func mossPicksGoFirstInItsOrder() {
        let uses = (0..<9).map { _ in MossQuickUse(id: "action:today", at: now) }
        let ctx = MossQuick.Context(mossPicks: ["cmd:/context", "action:stuck"])
        let ranked = MossQuick.ranked(MossQuick.items(), uses: uses, context: ctx, now: now)
        #expect(Array(ids(ranked).prefix(3)) == ["cmd:/context", "action:stuck", "action:today"])
    }

    @Test func mossPicksComeFromTheNewestMessageForADay() {
        let msgs = [
            MossMessage(id: "1", at: ago(days: 0.2), from: .moss, text: "a", quick: ["/status"]),
            MossMessage(id: "2", at: ago(days: 0.1), from: .moss, text: "b", quick: ["/usage", "waiting", "/login", "nope", "/usage"]),
            MossMessage(id: "3", at: ago(days: 0.05), from: .moss, text: "c"),
        ]
        #expect(MossQuick.mossPicks(in: msgs, now: now) == ["cmd:/usage", "action:needs-you"])
        let stale = [MossMessage(id: "1", at: ago(days: 2), from: .moss, text: "a", quick: ["/status"])]
        #expect(MossQuick.mossPicks(in: stale, now: now).isEmpty)
    }

    @Test func thePinWinsOverMessages() throws {
        let json = #"""
        {"schema":1,"quick":{"items":["today","/context"],"at":"2025-10-16T09:00:00Z"},
         "messages":[{"id":"1","at":"2025-10-16T09:30:00Z","from":"moss","text":"hi","quick":["/usage"]}]}
        """#
        let chat = try MossChat.decode(Data(json.utf8))
        #expect(chat.pin == MossChat.Pin(items: ["today", "/context"], at: parseTimestamp("2025-10-16T09:00:00Z")))
        let at = try #require(parseTimestamp("2025-10-16T12:00:00Z"))
        #expect(MossQuick.mossPicks(pin: chat.pin, in: chat.messages, now: at) == ["action:today", "cmd:/context"])
        #expect(MossQuick.mossPicks(pin: nil, in: chat.messages, now: at) == ["cmd:/usage"], "older cc-moss: the message's")
        let late = at.addingTimeInterval(86_400)
        #expect(MossQuick.mossPicks(pin: chat.pin, in: chat.messages, now: late).isEmpty, "a day old")
    }

    @Test func noPinFromAnOlderCLI() throws {
        #expect(try MossChat.decode(Data(#"{"schema":1,"messages":[]}"#.utf8)).pin == nil)
    }

    @Test func quickIsDecoded() throws {
        let json = #"{"schema":1,"messages":[{"id":"1","from":"moss","text":"hi","quick":["/usage","waiting",""]},{"id":"2","from":"moss","text":"x"}]}"#
        let messages = try MossChat.decode(Data(json.utf8)).messages
        #expect(messages[0].quick == ["/usage", "waiting"])
        #expect(messages[1].quick.isEmpty)
    }

    @Test func sentTextIsMatchedToItsItem() {
        #expect(MossQuick.item(sent: "/USAGE ")?.id == "cmd:/usage")
        #expect(MossQuick.item(sent: MossAction.all[1].text(.zh))?.id == "action:stuck")
        #expect(MossQuick.item(sent: MossAction.all[1].text(.en))?.id == "action:stuck")
        #expect(MossQuick.item(sent: "hello") == nil)
        #expect(MossQuick.item(sent: "/login") == nil)
    }

    @Test func aHotUsageIsNoticed() {
        let hot = MossMessage(id: "1", from: .moss, text: "  Current session\n  ████  83% used\n  Resets 5pm", command: "/usage")
        let calm = MossMessage(id: "2", from: .moss, text: "  Current session\n  ██  13% used\n  Resets 5pm", command: "/usage")
        #expect(MossQuick.usageHot(in: [hot]))
        #expect(!MossQuick.usageHot(in: [hot, calm]), "the latest one counts")
        #expect(!MossQuick.usageHot(in: []))
    }

    @Test func theLogIsCapped() {
        var uses: [MossQuickUse] = []
        for i in 0..<(MossQuickLog.cap + 5) { uses = MossQuickLog.adding("x\(i)", at: now, to: uses) }
        #expect(uses.count == MossQuickLog.cap)
        #expect(uses.last?.id == "x\(MossQuickLog.cap + 4)" && uses.first?.id == "x5")
    }

    @Test func theLogRoundTrips() throws {
        let defaults = try #require(UserDefaults(suiteName: "moss-quick-tests-\(UUID().uuidString)"))
        let uses = [MossQuickUse(id: "cmd:/usage", at: now)]
        MossQuickLog.save(uses, defaults)
        #expect(MossQuickLog.load(defaults) == uses)
    }
}
