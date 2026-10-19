import Foundation
import Testing
@testable import MossKit

@Suite struct MossUsageTests {
    let usage = MossUsage.parse(usageScreen)

    @Test func readsTheLimits() throws {
        let limits = try #require(usage).limits
        #expect(limits.map(\.label) == ["Current session", "Current week (all models)", "Current week (Sonnet)"])
        #expect(limits.map(\.percent) == [13, 52, 0])
        #expect(limits.map(\.resets) == ["1:49pm", "Oct 23 at 12:59am", "Oct 23 at 1am"], "the time zone is dropped")
    }

    @Test func limitLabelsAndLevels() throws {
        let limits = try #require(usage).limits
        #expect(limits[0].title(.zh) == "本次会话（5 小时）")
        #expect(limits[1].title(.zh) == "本周（全部模型）")
        #expect(limits[2].title(.zh) == "本周（Sonnet）")
        #expect(limits[1].title(.en) == "This week (all models)")
        #expect(MossUsage.Limit(label: "Something else", percent: 1).title(.zh) == "Something else")
        #expect(limits.map(\.level) == [.calm, .warm, .calm])
        #expect(MossUsage.Limit(label: "x", percent: 80).level == .hot)
        #expect(MossUsage.Limit(label: "x", percent: 79).level == .warm)
    }

    @Test func readsTheSession() throws {
        let s = try #require(usage?.session)
        #expect(s.cost == "$1.2400")
        #expect(s.wallDuration == "41m 5s" && s.apiDuration == "3m 12s")
        #expect(s.added == 120 && s.removed == 8)
        #expect(s.input == "18.2k" && s.output == "4.1k")
        #expect(!s.isEmpty)
    }

    @Test func readsTheNewerSessionShape() throws {
        let text = """
        ────────────────────────────────────────────────────────────────────────────────────────────────────
          Settings  Status   Config   Usage   Stats

          Session

          Total cost:            $0.4100
          Total duration (API):  21s
          Total duration (wall): 50s
          Total code changes:    4 lines added, 0 lines removed
          Usage by model:
              claude-haiku-9-9:  1.2k input, 353 output, 0 cache read, 0 cache write ($0.0003)
               claude-opus-9-9:  514 input, 877 output, 246.8k cache read, 22.7k cache write ($0.4097)
          Prompt cache (main):   4 requests · 89% of input tokens from cache · no misses · warm (1h TTL,
          last activity 28s ago)


          Plugin skill-listing footprint
          What each plugin's skill descriptions add to the system prompt.

          example-plugin              1 skill · ~30 tok/turn

          Current session
          ███████                                            14% used
          Resets 1:49pm (Europe/Berlin)
        """
        let u = try #require(MossUsage.parse(text))
        let s = try #require(u.session)
        #expect(s.cost == "$0.4100" && s.added == 4 && s.removed == 0)
        #expect(s.input == nil && s.output == nil)
        #expect(s.models == [
            MossUsage.ModelUsage(model: "claude-haiku-9-9", input: "1.2k", output: "353", cost: "$0.0003"),
            MossUsage.ModelUsage(model: "claude-opus-9-9", input: "514", output: "877", cost: "$0.4097"),
        ])
        #expect(s.cache == "4 requests · 89% of input tokens from cache · no misses · warm (1h TTL, last activity 28s ago)")
        #expect(s.cacheHit == 89)
        #expect(s.cacheDetail == "4 requests · no misses · warm (1h TTL, last activity 28s ago)")
        #expect(!s.isEmpty)
        #expect(u.limits.map(\.percent) == [14], "the footprint section is skipped")
    }

    @Test func anIdleSessionIsEmpty() throws {
        let text = """
          Session
          Total cost:            $0.0000
          Total duration (API):  0s
          Total duration (wall): 2s
          Total code changes:    0 lines added, 0 lines removed
          Usage:                 0 input, 0 output, 0 cache read, 0 cache write
        """
        #expect(try #require(MossUsage.parse(text)?.session).isEmpty)
    }

    @Test func readsTheInsights() throws {
        let u = try #require(usage)
        #expect(u.insights.map(\.percent) == [98, 85, 16, 7])
        #expect(u.insights[0].headline == "came from subagent-heavy sessions")
        #expect(u.insights[0].hint.hasPrefix("Each subagent runs its own requests."))
        #expect(u.insights[0].hint.hasSuffix("simpler subagents."))
        #expect(u.insightsPeriod?.hasPrefix("Last 24h") == true)
    }

    @Test func knownHeadlinesAreTranslated() throws {
        let u = try #require(usage)
        #expect(u.insights[0].title(.zh) == "来自大量用子代理的会话")
        #expect(u.insights[1].title(.zh) == "发生在上下文超过 150k 时")
        #expect(u.insights[2].title(.zh) == "发生在 4 个以上会话同时运行时")
        #expect(u.insights[3].title(.zh) == "came from something new", "unknown phrases stay as written")
        #expect(u.insights[0].title(.en) == "Came from subagent-heavy sessions")
    }

    @Test func readsTheTablesRanked() throws {
        let tables = try #require(usage).tables
        #expect(tables.map(\.title) == ["Skills", "Subagents", "MCP servers"])
        #expect(tables[0].rows.map(\.name) == ["/deploy", "/review"])
        #expect(tables[1].rows.first == MossUsage.Share(name: "general-purpose", percent: 38))
        #expect(tables[2].title(.zh) == "MCP 服务")
    }

    @Test func readsTheLoops() throws {
        let loop = try #require(usage?.loops.first)
        #expect(loop.name == "nightly report check (once a day, rea…")
        #expect(loop.every == "at 10:17" && loop.runs == "1" && loop.tokens == "206.7k" && loop.lastRun == "1h ago")
    }

    @Test func everySectionIsOptional() throws {
        let only = """
          Current session
          ████████████████████████████████████████           83% used
          Resets 5pm
        """
        let u = try #require(MossUsage.parse(only))
        #expect(u.limits.count == 1 && u.limits[0].level == .hot)
        #expect(u.session == nil && u.insights.isEmpty && u.tables.isEmpty && u.loops.isEmpty && u.plan == nil)
    }

    @Test func aThirdPartyOneLinerIsNotAScreen() {
        #expect(MossUsage.parse("/usage is only available for subscription plans.") == nil)
        #expect(MossUsage.parse("That command printed nothing.") == nil)
        #expect(MossUsage.parse("") == nil)
    }

    @Test func thePlanIsReadWhenShown() throws {
        #expect(MossUsage.parse("Opus 5.5 · Claude Max\n" + usageScreen)?.plan == "Claude Max")
        #expect(try #require(usage).plan == nil)
    }

    @Test func theRawViewDropsTheFrame() {
        let raw = MossScreenText.withoutChrome(statusScreen)
        #expect(raw.hasPrefix("Version:"), "the shared margin goes too")
        #expect(!raw.contains("Settings  Status") && !raw.contains("Esc to cancel") && !raw.contains("────"))
        #expect(MossScreenText.withoutChrome("That command printed nothing.") == "That command printed nothing.")
    }

    @Test func aStatusScreenIsNotUsage() {
        #expect(MossUsage.parse(statusScreen) == nil)
    }
}
