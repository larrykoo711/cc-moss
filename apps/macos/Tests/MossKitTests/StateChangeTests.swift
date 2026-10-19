import Testing
@testable import MossKit

/// What in the state folder is worth a refresh (user 2026-10-19: the
/// session list refreshed about once a second).
@Suite struct StateChangeTests {
    private let dir = "/Users/alex/.local/state/cc-moss"

    @Test func theRegistryAndQuestionsMatter() {
        #expect(StateChange.matters("\(dir)/registry.changed", in: dir))
        #expect(StateChange.matters("\(dir)/asks/9c8d0e1f.json", in: dir))
        #expect(StateChange.matters("\(dir)/moss/chat.jsonl", in: dir))
    }

    @Test func sqliteSideFilesLogsAndOurOwnHeartbeatDoNot() {
        // Opening the registry, even to read, makes and removes these.
        #expect(!StateChange.matters("\(dir)/registry.db", in: dir))
        #expect(!StateChange.matters("\(dir)/registry.db-wal", in: dir))
        #expect(!StateChange.matters("\(dir)/registry.db-shm", in: dir))
        #expect(!StateChange.matters("\(dir)/registry.db-journal", in: dir))
        #expect(!StateChange.matters("\(dir)/hook.log", in: dir))
        #expect(!StateChange.matters("\(dir)/asks/takers/desktop.alive", in: dir))
        #expect(!StateChange.matters("\(dir)/name-scan.json", in: dir))
        #expect(!StateChange.matters("\(dir)/adopt.stamp", in: dir))
        #expect(!StateChange.matters("\(dir)/daemon/daemon.log", in: dir))
        #expect(!StateChange.matters("\(dir)/moss/SESSIONS.md", in: dir))
        #expect(!StateChange.matters("/elsewhere/registry.changed", in: dir))
    }

    @Test func anOlderCcMossIsFollowedThroughTheDatabase() {
        #expect(StateChange.matters("\(dir)/registry.db", in: dir, announced: false))
        #expect(!StateChange.matters("\(dir)/registry.db-wal", in: dir, announced: false))
    }

    @Test func slowerWhileThePanelIsClosed() {
        #expect(StateChange.gap(panelShown: true) == 1.5)
        #expect(StateChange.gap(panelShown: false) == 5)
    }
}
