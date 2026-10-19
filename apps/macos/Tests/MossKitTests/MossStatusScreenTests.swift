import Foundation
import Testing
@testable import MossKit

@Suite struct MossStatusScreenTests {
    let status = MossStatusScreen.parse(statusScreen)

    @Test func readsTheHeader() throws {
        let s = try #require(status)
        #expect(s.version == "2.1.303")
        #expect(s.model == "Opus 5.5")
        #expect(s.modelNote == "Best for everyday, complex tasks")
        #expect(s.plan == "Claude Max")
    }

    @Test func readsTheAccount() throws {
        let s = try #require(status)
        #expect(s.value("Organization") == "Example Org")
        #expect(s.value("Email") == "alex@example.com")
    }

    @Test func aWrappedValueIsJoined() throws {
        let cwd = try #require(status?.value("cwd"))
        #expect(cwd == "/Users/alex/work/projects/a-very-long-folder-name-that-the-terminal-will-wrap-somewhere-in-the-middle/app")
    }

    @Test func wrappedWordsKeepTheirSpace() throws {
        let text = """
          Version:           2.1.303
          Login method:      Claude Max account
          Model:             Default (Opus 5.5   Best for everyday,
                             complex tasks)
        """
        let s = try #require(MossStatusScreen.parse(text))
        #expect(s.value("Model") == "Default (Opus 5.5   Best for everyday, complex tasks)")
        #expect(s.model == "Opus 5.5")
    }

    @Test func noNameYet() throws {
        #expect(try #require(status).sessionName == nil)
        let named = try #require(MossStatusScreen.parse(statusScreen.replacingOccurrences(of: "/rename to add a name", with: "billing-fix")))
        #expect(named.sessionName == "billing-fix")
    }

    @Test func connections() throws {
        let s = try #require(status)
        #expect(s.ide?.connected == false)
        #expect(s.ide?.text == "Not connected to Visual Studio Code")
        #expect(s.mcp == "1 disabled")
        #expect(s.value("Cloud sessions") == "GitHub connected")
        let on = try #require(MossStatusScreen.parse(statusScreen.replacingOccurrences(of: "✘ Not connected to", with: "✔ Connected to")))
        #expect(on.ide?.connected == true)
    }

    @Test func peerAddressIsDroppedAndTheRestIsOther() throws {
        let s = try #require(status)
        #expect(s.value("Peer address") == nil)
        #expect(s.others.map(\.key) == ["Auto mode server"], "empty values are left out")
    }

    @Test func missingKeysAreFine() throws {
        let text = """
          Version:           2.1.303
          Login method:      Anthropic API key
          Model:             claude-sonnet-5
        """
        let s = try #require(MossStatusScreen.parse(text))
        #expect(s.plan == "Anthropic API key")
        #expect(s.model == "claude-sonnet-5" && s.modelNote == nil)
        #expect(s.value("Email") == nil && s.ide == nil && s.sessionName == nil)
    }

    @Test func aOneLinerIsNotAStatusScreen() {
        #expect(MossStatusScreen.parse("Unknown command: /status") == nil)
        #expect(MossStatusScreen.parse("That command printed nothing.") == nil)
    }

    @Test func aUsageScreenIsNotStatus() {
        #expect(MossStatusScreen.parse(usageScreen) == nil)
    }
}
