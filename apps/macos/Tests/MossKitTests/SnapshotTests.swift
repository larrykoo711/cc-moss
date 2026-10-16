import Foundation
import Testing
@testable import MossKit

@Suite struct SnapshotTests {
    @Test func decodesSample() throws {
        let snap = try Snapshot.decode(snapshotData([
            sessionJSON(id: "A", busy: true, note: "fix the login page", worktree: ["root": "/w", "repo": "/r"]),
        ]))
        #expect(snap.stateDir == "/home/u/.local/state/cc-moss")
        let s = try #require(snap.sessions.first)
        #expect(s.name == "job-a")
        #expect(s.busy)
        #expect(s.notes.note == "fix the login page")
        #expect(s.tmux?.sessionName == "host-a_job-a")
        #expect(s.worktree == Session.Worktree(root: "/w", repo: "/r"))
        #expect(s.lastActiveAt == parseTimestamp("2026-10-16T10:00:00Z"))
    }

    @Test func toleratesMissingOptionalFields() throws {
        // An older cc-moss (0.1.x) prints neither state_dir nor worktree.
        var s = sessionJSON()
        s.removeValue(forKey: "worktree")
        s.removeValue(forKey: "busy")
        s.removeValue(forKey: "last_prompt")
        let snap = try Snapshot.decode(snapshotData([s], stateDir: nil))
        #expect(snap.stateDir == nil)
        #expect(snap.sessions.first?.worktree == nil)
        #expect(snap.sessions.first?.busy == false)
    }

    @Test func decodesAttention() throws {
        let snap = try Snapshot.decode(snapshotData([
            sessionJSON(id: "A", attention: "permission"),
            sessionJSON(id: "B"),
        ]))
        #expect(snap.sessions[0].attention == "permission")
        #expect(snap.sessions[0].needsYou)
        #expect(snap.sessions[1].attention == nil)
        #expect(!snap.sessions[1].needsYou)
    }

    @Test func attentionMissingFromOlderCLI() throws {
        var s = sessionJSON()
        s.removeValue(forKey: "attention")
        let snap = try Snapshot.decode(snapshotData([s]))
        #expect(snap.sessions.first?.attention == nil)
    }

    @Test func rejectsNewerSchema() {
        #expect(throws: SnapshotError.newerSchema(2)) {
            try Snapshot.decode(snapshotData([["whatever": 1]], schema: 2))
        }
    }

    @Test func rejectsGarbage() {
        #expect(throws: SnapshotError.self) {
            try Snapshot.decode(Data("not json".utf8))
        }
    }
}
