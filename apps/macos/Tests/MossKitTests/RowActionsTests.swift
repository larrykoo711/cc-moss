import Foundation
import Testing
@testable import MossKit

@Suite struct RowActionsTests {
    let list = sessions([
        sessionJSON(id: "A", name: "live", status: "running"),
        sessionJSON(id: "B", name: "broken", status: "interrupted"),
        sessionJSON(id: "C", name: "outside", status: "running", tmux: false),
    ])

    @Test func aLiveSessionShowsRenameMessageAndOpen() {
        let a = RowActions(list[0], hopeless: false)
        #expect(a.inline == [.message, .open, .rename])
        #expect(a.more == [[.note], [.copyCommand, .copyPath]])
    }

    @Test func aBrokenSessionLeadsWithBringBack() {
        let a = RowActions(list[1], hopeless: false)
        #expect(a.inline == [.bringBack])
        #expect(a.more == [[.note], [.copyRestoreCommand, .copyPath], [.close, .archive]])
    }

    @Test func nothingToBringBackOffersToStopReminding() {
        let a = RowActions(list[1], hopeless: true)
        #expect(a.inline == [.stopReminding])
        #expect(!a.all.joined().contains(.close), "stopping is already the main action")
        #expect(a.all.joined().contains(.archive))
    }

    @Test func outsideTmuxItCanMoveInOrGiveItsFolder() {
        let a = RowActions(list[2], hopeless: false)
        #expect(a.inline == [.adopt, .copyPath])
        #expect(a.more == [[.adopt], [.note]])
    }

    @Test func theFullMenuHasEveryActionOnce() {
        for s in list {
            let a = RowActions(s, hopeless: false)
            let every = a.all.joined()
            #expect(Set(every).count == every.count)
            #expect(Set(every) == Set(a.inline + a.more.joined()))
            #expect(a.all.first?.first == a.primary)
            #expect(a.inline.count <= RowActions.inlineLimit)
        }
    }

    let more = sessions([
        with(sessionJSON(id: "R", name: "stale"), ["provider_restart": "removed"]),
        with(sessionJSON(id: "Q", name: "queued", busy: true), ["provider_restart": "model", "restart_queued": true]),
        with(sessionJSON(id: "Z", name: "put-away", status: "closed"), ["archived_at": "2026-09-01T00:00:00Z"]),
        with(sessionJSON(id: "O", name: "outside-stale", tmux: false), ["provider_restart": "removed"]),
    ])

    @Test func rowActionsRestartAStaleSessionInline() {
        let a = RowActions(more[0], hopeless: false)
        #expect(a.inline == [.restart, .message, .open], "rename steps back into ⋯ to make room")
        #expect(a.more.joined().contains(.rename))
        #expect(!a.more.joined().contains(.restart))
    }

    @Test func rowActionsDontRestartAQueuedSessionAgain() {
        let a = RowActions(more[1], hopeless: false)
        #expect(!a.all.joined().contains(.restart))
    }

    @Test func rowActionsDontRestartOutsideTmux() {
        let a = RowActions(more[3], hopeless: false)
        #expect(!a.all.joined().contains(.restart), "cc-moss refuses: move it into tmux first")
    }

    @Test func rowActionsLeaveMossToCCMoss() {
        var moss = more[0]
        moss.role = "moss"
        #expect(!RowActions(moss, hopeless: false).all.joined().contains(.restart))
    }

    @Test func rowActionsForAnArchivedSession() {
        let a = RowActions(more[2], hopeless: false)
        #expect(a.primary == .unarchive)
        #expect(a.inline == [.cleanUp, .unarchive])
        #expect(!a.all.joined().contains(.archive))
    }

    @Test func rowActionsArchiveOnlyWhatIsNotRunning() {
        #expect(!RowActions(list[0], hopeless: false).all.joined().contains(.archive))
        #expect(RowActions(list[1], hopeless: false).all.joined().contains(.archive))
    }

    @Test func rowActionsNewCasesKeepTheRules() {
        for s in more {
            let a = RowActions(s, hopeless: false)
            let every = a.all.joined()
            #expect(Set(every).count == every.count)
            #expect(Set(every) == Set(a.inline + a.more.joined()))
            #expect(a.all.first?.first == a.primary)
            #expect(a.inline.count <= RowActions.inlineLimit)
        }
    }
}
