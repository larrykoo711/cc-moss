import Foundation
import Testing
@testable import MossKit

@Suite struct ProviderBannerTests {
    let now = parseTimestamp("2026-10-20T12:00:00Z")!

    let flagged = sessions([
        with(sessionJSON(id: "A", name: "api"), ["provider_restart": "removed", "restart_queued": false]),
        with(sessionJSON(id: "B", name: "web", busy: true), ["provider_restart": "model", "restart_queued": true]),
        sessionJSON(id: "C", name: "docs"),
    ])
    let calm = sessions([sessionJSON(id: "C", name: "docs")])

    private func page(_ json: String) throws -> [SessionEvent] {
        try EventPage.decode(Data(json.utf8)).events
    }

    private func change(seq: Int, minutesAgo: Double, live: Bool, host: String? = "api.example.com",
                        restart: [String] = []) -> SessionEvent {
        SessionEvent(seq: seq, slotId: "", kind: ProviderChange.kind, at: now.addingTimeInterval(-minutesAgo * 60),
                     provider: ProviderChange(host: host, live: live, restartNeeded: restart))
    }

    @Test func theEventDecodesFromEventsJSON() throws {
        let events = try page(#"{"schema":1,"last_seq":9,"events":[{"seq":9,"slot_id":"","kind":"provider-changed","at":"2026-10-20T11:58:00Z","detail":{"host":"api.example.com","live":false,"restart_needed":["01A","01B"]}}]}"#)
        #expect(events.first?.provider == ProviderChange(host: "api.example.com", live: false, restartNeeded: ["01A", "01B"]))
        #expect(events.first?.slotId == "")
    }

    @Test func aGlobalEventWithoutSlotStillDecodes() throws {
        let events = try page(#"{"schema":1,"last_seq":3,"events":[{"seq":3,"kind":"archived","detail":{"count":5,"by":"auto"}}]}"#)
        #expect(events.count == 1 && events[0].provider == nil)
    }

    @Test func otherEventsAreNoProviderChange() throws {
        let events = try page(#"{"schema":1,"last_seq":4,"events":[{"seq":4,"slot_id":"01A","kind":"needs_you","detail":{"reason":"input"}}]}"#)
        #expect(events[0].provider == nil && events[0].reason == "input")
    }

    @Test func providerBannerIsHiddenWithoutASwitch() {
        #expect(ProviderBanner().state(now: now, sessions: flagged) == .hidden)
    }

    @Test func providerBannerSaysALiveSwitchTakesEffect() {
        var b = ProviderBanner()
        b.take([change(seq: 1, minutesAgo: 2, live: true)], now: now)
        #expect(b.state(now: now, sessions: calm) == .live(host: "api.example.com"))
    }

    @Test func providerBannerCountsSessionsThatNeedARestart() {
        var b = ProviderBanner()
        b.take([change(seq: 1, minutesAgo: 1, live: false, restart: ["A", "B"])], now: now)
        #expect(b.state(now: now, sessions: flagged) == .needsRestart(count: 1))
    }

    @Test func providerBannerTurnsLiveOnceEveryoneRestarted() {
        var b = ProviderBanner()
        b.take([change(seq: 1, minutesAgo: 1, live: false, restart: ["A"])], now: now)
        #expect(b.state(now: now, sessions: calm) == .live(host: "api.example.com"))
    }

    @Test func providerBannerLeavesMossToCCMoss() {
        var moss = flagged[0]
        moss.role = "moss"
        var b = ProviderBanner()
        b.take([change(seq: 1, minutesAgo: 1, live: false)], now: now)
        #expect(b.state(now: now, sessions: [moss]) == .live(host: "api.example.com"))
    }

    @Test func providerBannerGoesAfterTenMinutes() {
        var b = ProviderBanner()
        b.take([change(seq: 1, minutesAgo: 9.9, live: true)], now: now)
        #expect(b.state(now: now, sessions: calm) != .hidden)
        #expect(b.state(now: now.addingTimeInterval(10), sessions: calm) == .hidden)
    }

    @Test func providerBannerStaysClosedUntilTheNextSwitch() {
        var b = ProviderBanner()
        b.take([change(seq: 1, minutesAgo: 1, live: true)], now: now)
        b.dismiss()
        #expect(b.state(now: now, sessions: calm) == .hidden)
        b.take([change(seq: 2, minutesAgo: 0, live: false, restart: ["A"])], now: now)
        #expect(b.state(now: now, sessions: flagged) == .needsRestart(count: 1))
    }

    @Test func providerBannerFollowsTheNewestSwitch() {
        var b = ProviderBanner()
        b.take([change(seq: 5, minutesAgo: 1, live: true, host: "new.example.com"),
                change(seq: 4, minutesAgo: 3, live: false, host: "old.example.com")], now: now)
        #expect(b.state(now: now, sessions: calm) == .live(host: "new.example.com"))
        #expect(b.state(now: now, sessions: [flagged[1]]) == .queued(count: 1))
    }

    @Test func providerBannerWithoutATimeCountsFromWhenItArrived() {
        var b = ProviderBanner()
        b.take([SessionEvent(seq: 1, slotId: "", kind: ProviderChange.kind, provider: ProviderChange(host: nil, live: true))], now: now)
        #expect(b.state(now: now.addingTimeInterval(60), sessions: calm) == .live(host: nil))
        #expect(b.state(now: now.addingTimeInterval(601), sessions: calm) == .hidden)
    }

    @Test func providerBannerWordsInChinese() {
        L.$override.withValue(.zh) {
            #expect(ProviderBanner.text(.live(host: "api.example.com")) == "渠道设置已改为 api.example.com，请确认各会话的模型")
            #expect(ProviderBanner.text(.needsRestart(count: 3)) == "3 个会话要重开才能用上新供应商")
        }
    }

    @Test func providerBannerWordsInEnglish() {
        L.$override.withValue(.en) {
            #expect(ProviderBanner.text(.live(host: "api.example.com")) == "Provider settings changed to api.example.com. Check each session’s model.")
            #expect(ProviderBanner.text(.needsRestart(count: 1)) == "1 session needs a restart to use the new provider")
            #expect(ProviderBanner.text(.hidden) == nil)
        }
    }

    @Test func sessionsCarryTheirRestartFlags() throws {
        #expect(flagged[0].providerRestart == .removed && !flagged[0].restartQueued)
        #expect(flagged[1].providerRestart == .model && flagged[1].restartQueued)
        #expect(flagged[2].providerRestart == nil && !flagged[2].restartQueued)
        let snap = try Snapshot.decode(Data(#"{"schema":1,"provider":{"host":"api.example.com"},"sessions":[]}"#.utf8))
        #expect(snap.providerHost == "api.example.com")
    }

    @Test func restartRunsOneOrEveryStaleSession() async throws {
        let runner = Recorder(stdout: #"{"schema":1,"results":[{"name":"api","session_id":"s","result":"restarted"},{"name":"web","session_id":"t","result":"queued"}]}"#)
        let client = MossClient(executable: "/bin/cc-moss", runner: runner)
        _ = try await client.restart(["api"])
        let all = try await client.restartStale()
        #expect(await runner.calls == [["restart", "--json", "--", "api"], ["restart", "--stale", "--provider-default", "--json"]])
        L.$override.withValue(.en) {
            #expect(Restart.summary(all) == "Restarted 1 · 1 queued")
            #expect(Restart.summary([all[1]]) == "web is queued.")
        }
    }

    @Test func restartRefusalsInWords() {
        L.$override.withValue(.en) {
            #expect(Restart.summary([BatchResult(name: "x", outcome: "refused", reason: "not-in-tmux")])
                == "x isn't in tmux. Move it in first.")
        }
    }
}
