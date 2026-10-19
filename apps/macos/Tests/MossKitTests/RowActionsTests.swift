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
        #expect(a.more == [[.note], [.copyRestoreCommand, .copyPath], [.close]])
    }

    @Test func nothingToBringBackOffersToStopReminding() {
        let a = RowActions(list[1], hopeless: true)
        #expect(a.inline == [.stopReminding])
        #expect(!a.all.joined().contains(.close), "stopping is already the main action")
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
}
