import Foundation
import Testing
@testable import MossKit

@Suite struct MossCommandTests {
    @Test func aCommandResultIsDecoded() throws {
        let json = #"{"schema":1,"messages":[{"id":"1","from":"moss","text":"Skills % of usage","command":"/usage"}]}"#
        let m = try MossChat.decode(Data(json.utf8)).messages[0]
        #expect(m.command == "/usage")
        #expect(!m.refused)
    }

    @Test func aRefusalIsDecoded() throws {
        let json = #"{"schema":1,"messages":[{"id":"1","from":"moss","text":"Do it in the terminal","command":"/login","refused":true}]}"#
        let m = try MossChat.decode(Data(json.utf8)).messages[0]
        #expect(m.command == "/login")
        #expect(m.refused)
    }

    @Test func ordinaryMessagesHaveNoCommand() throws {
        let json = #"{"schema":1,"messages":[{"id":"1","from":"moss","text":"hi","command":""}]}"#
        let m = try MossChat.decode(Data(json.utf8)).messages[0]
        #expect(m.command == nil)
        #expect(!m.refused)
    }

    @Test func theAllowlist() {
        #expect(MossCommand.all.map(\.name) == ["/usage", "/status", "/context", "/cost", "/stats"])
        #expect(!MossCommand.isAllowed("/clear") && !MossCommand.isAllowed("/compact"), "Moss's context is managed for it")
        #expect(MossCommand.isAllowed("/usage"))
        #expect(MossCommand.isAllowed("/USAGE "))
        #expect(!MossCommand.isAllowed("/login"))
        #expect(!MossCommand.isAllowed("usage"))
    }

    @Test func everyCommandSaysWhatItDoesInBothLanguages() {
        for c in MossCommand.all {
            #expect(!c.detail(.en).isEmpty && !c.detail(.zh).isEmpty, "\(c.name)")
            #expect(c.name.hasPrefix("/"))
        }
        #expect(Set(MossCommand.all.map(\.name)).count == MossCommand.all.count)
    }

    @Test func suggestionsFollowThePrefix() {
        #expect(MossCommand.suggestions(for: "/").count == MossCommand.all.count)
        #expect(MossCommand.suggestions(for: "/c").map(\.name) == ["/context", "/cost"])
        #expect(MossCommand.suggestions(for: "/co").map(\.name) == ["/context", "/cost"])
        #expect(MossCommand.suggestions(for: "/cl").isEmpty, "no /clear")
        #expect(MossCommand.suggestions(for: "/St").map(\.name) == ["/status", "/stats"])
    }

    @Test func noSuggestionsOnceThereIsASpaceOrNoSlash() {
        #expect(MossCommand.suggestions(for: "/usage now").isEmpty)
        #expect(MossCommand.suggestions(for: "usage").isEmpty)
        #expect(MossCommand.suggestions(for: "").isEmpty)
        #expect(MossCommand.suggestions(for: "/zzz").isEmpty)
    }

    @Test func theAccountIsRead() throws {
        let json = #"{"schema":1,"enabled":true,"running":true,"account":"api"}"#
        #expect(try MossStatus.decode(Data(json.utf8)).account == .api)
        let older = #"{"schema":1,"enabled":true,"running":true}"#
        #expect(try MossStatus.decode(Data(older.utf8)).account == .unknown)
        let newer = #"{"schema":1,"enabled":true,"running":true,"account":"enterprise"}"#
        #expect(try MossStatus.decode(Data(newer.utf8)).account == .unknown)
    }

    private func names(_ account: MossAccount) -> [String] {
        MossCommand.suggestions(for: "/", account: account).map(\.name)
    }

    @Test func aSubscriptionGetsUsageNotCost() {
        #expect(names(.subscription) == ["/usage", "/status", "/context", "/stats"])
    }

    @Test func anAPIKeyGetsCostNotUsage() {
        #expect(names(.api) == ["/status", "/context", "/cost", "/stats"])
    }

    @Test func aCloudAccountGetsCostNotUsage() {
        #expect(names(.cloud) == ["/status", "/context", "/cost", "/stats"])
    }

    @Test func unknownShowsEverything() {
        #expect(names(.unknown) == MossCommand.all.map(\.name))
    }

    @Test func aCommandThatDoesntFitIsStillACommand() {
        #expect(MossCommand.suggestions(for: "/us", account: .api).isEmpty)
        #expect(MossCommand.isAllowed("/usage"), "typed by hand, it's still sent")
    }

    @Test func aSlashMakesItLookLikeACommand() {
        #expect(MossCommand.looksLikeCommand("/usage"))
        #expect(MossCommand.looksLikeCommand("  /context"))
        #expect(!MossCommand.looksLikeCommand("what's /usage?"))
    }
}
