import Foundation
import Testing
@testable import MossKit

/// The App review's fixes (R-APP): archived mention cards, long permissions,
/// the receipt's baseline, the provider banner's count.
@Suite struct ReviewFixTests {
    // MARK: M1 archived mentions

    let archived = sessions([with(sessionJSON(id: "0AR", name: "old-one", status: "closed"),
                                  ["archived_at": "2026-09-01T00:00:00Z"])])

    @Test func anArchivedMentionIsNotGone() {
        let card = MossSessionCard.cards(["0AR"], among: archived)[0]
        #expect(!card.isGone)
        #expect(card.main == .unarchive)
        #expect(card.shown)
    }

    @Test func anArchivedMentionIsNotPinned() {
        #expect(!MossSessionCard.cards(["0AR"], among: archived)[0].canPin)
    }

    @Test func archivedMossHasNoButton() {
        var moss = archived[0]
        moss.role = "moss"
        #expect(MossSessionCard.cards(["0AR"], among: [moss])[0].main == nil)
    }

    // MARK: M2 long permissions

    private func permission(_ preview: String) -> WaitingRow {
        WaitingRow(id: "x", session: nil, title: "x", item: .ask("p"), want: .permission(tool: "Bash", detail: preview),
                   more: 0, since: Date())
    }

    @Test func aShortCommandFitsItsLines() {
        #expect(!permission("npm test").needsShowAll)
    }

    @Test func aLongCommandOffersShowAll() {
        #expect(permission(String(repeating: "rm -rf build/ && ", count: 12)).needsShowAll)
    }

    @Test func manyShortLinesAlsoOverflow() {
        #expect(permission("a\nb\nc\nd").needsShowAll)
    }

    @Test func questionsNeverOfferShowAll() {
        let q = WaitingRow(id: "x", session: nil, title: "x", item: .ask("q"),
                           want: .question(text: String(repeating: "long ", count: 80), options: []), more: 0, since: Date())
        #expect(!q.needsShowAll)
    }

    // MARK: L3 receipt baseline

    let t0 = parseTimestamp("2026-10-18T10:00:00Z")!

    @Test func comingBackCountsNothingThatWentWhileAway() {
        var r = WaitingReceipt()
        r.rebaseline([.ask("p1"): "s1"])
        // Away: p1 was answered on the To Do page. Back on the page:
        r.rebaseline([:])
        #expect(r.receipt(now: t0) == nil)
    }

    @Test func comingBackEndsAnOldReceipt() {
        var r = WaitingReceipt()
        r.update([.ask("p1"): "s1"], live: ["s1"], now: t0)
        r.update([:], live: ["s1"], now: t0)
        #expect(r.receipt(now: t0) == 1)
        r.rebaseline([:])
        #expect(r.receipt(now: t0) == nil)
    }

    @Test func aSessionThatEndedIsNotHandled() {
        var r = WaitingReceipt()
        r.rebaseline([.ask("p1"): "s1", .reply("01B"): "s2"])
        r.update([:], live: ["s2"], now: t0)
        #expect(r.receipt(now: t0) == 1, "only the one still running was handled")
    }

    @Test func nothingHandledWhenEveryoneEnded() {
        var r = WaitingReceipt()
        r.rebaseline([.ask("p1"): "s1"])
        r.update([:], live: [], now: t0)
        #expect(r.receipt(now: t0) == nil)
    }

    // MARK: L4 banner count

    let now = parseTimestamp("2026-10-20T12:00:00Z")!
    let mixed = sessions([
        with(sessionJSON(id: "A", name: "ready"), ["provider_restart": "removed"]),
        with(sessionJSON(id: "B", name: "queued", busy: true), ["provider_restart": "model", "restart_queued": true]),
        with(sessionJSON(id: "C", name: "outside", tmux: false), ["provider_restart": "removed"]),
    ])

    private func banner(_ list: [Session]) -> ProviderBanner.State {
        var b = ProviderBanner()
        b.take([SessionEvent(seq: 1, slotId: "", kind: ProviderChange.kind, at: now,
                             provider: ProviderChange(host: "h", live: false))], now: now)
        return b.state(now: now, sessions: list)
    }

    @Test func providerBannerCountsOnlyWhatRestartAllCanRestart() {
        #expect(banner(mixed) == .needsRestart(count: 1, outside: 1))
    }

    @Test func providerBannerWithOnlyOutsideTmuxHasNothingToRestart() {
        #expect(banner([mixed[2]]) == .needsRestart(count: 0, outside: 1))
        L.$override.withValue(.zh) {
            #expect(ProviderBanner.text(.needsRestart(count: 0, outside: 1)) == "1 个不在 tmux 里，先移进去")
        }
    }

    @Test func providerBannerWithOnlyQueuedStaysPending() {
        #expect(banner([mixed[1]]) == .queued(count: 1))
    }

    @Test func providerBannerWordsBothParts() {
        L.$override.withValue(.en) {
            #expect(ProviderBanner.text(.needsRestart(count: 2, outside: 1))
                == "2 sessions need a restart to use the new provider. 1 not in tmux: move it in first")
        }
    }
}
