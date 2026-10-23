import Foundation
import Testing
@testable import MossKit

@Suite struct SearchIndexesTests {
    @Test(arguments: ["sessionId", "provider", "providerRestart", "failure", "queue", "notes", "tmux"])
    func returnsTheCurrentSessionWhenOnlyNonTextFieldsChange(_ field: String) {
        let original = sessions([sessionJSON()])[0]
        var current = original
        switch field {
        case "sessionId": current.sessionId = "replacement-session"
        case "provider": current.providerModel = ProviderModel(model: "model-b", source: "settings", revision: "revision-b", uncertain: false)
        case "providerRestart": current.providerRestart = .model
        case "failure": current.apiFailure = APIFailure(kind: "generic_api", status: 503, model: "model-b", atMs: 123)
        case "queue": current.restartQueued = true
        case "notes": current.notes.goal = "new goal"
        default: current.tmux?.paneId = "%99"
        }
        let cache = SearchIndexes()
        _ = cache.live([original], folders: [:]).search(original.title)
        _ = cache.archived([original]).search(original.title)
        _ = cache.closed([original]).search(original.title)
        #expect(cache.live([current], folders: [:]).search(current.title).first?.session == current)
        #expect(cache.archived([current]).search(current.title).first?.session == current)
        #expect(cache.closed([current]).search(current.title).first?.session == current)
    }

    @Test func clearedFailureAndQueueDoNotLingerInSearchResults() {
        let current = sessions([sessionJSON()])[0]
        var previous = current
        previous.apiFailure = APIFailure(kind: "generic_api", status: 503, model: nil, atMs: 123)
        previous.restartQueued = true
        previous.providerRestart = .model
        let cache = SearchIndexes()
        _ = cache.live([previous], folders: [:])
        #expect(cache.live([current], folders: [:]).search(current.title).first?.session == current)
    }

    @Test func folderLabelChangesWithoutSessionChanges() {
        let session = sessions([sessionJSON()])[0]
        let cache = SearchIndexes()
        _ = cache.live([session], folders: [session.id: "old-folder-label"])
        let index = cache.live([session], folders: [session.id: "new-folder-label"])
        #expect(index.search("new-folder-label").first?.session == session)
        #expect(index.search("old-folder-label").isEmpty)
    }

    @Test func updatedNotesAreSearchableWithoutActivityChanges() {
        var session = sessions([sessionJSON()])[0]
        let cache = SearchIndexes()
        _ = cache.live([session], folders: [:])
        session.notes.state = "unique-search-state"
        session.notes.goal = "unique-search-goal"
        let index = cache.live([session], folders: [:])
        #expect(index.search("unique-search-state").first?.session == session)
        #expect(index.search("unique-search-goal").first?.session == session)
    }

    @Test func removalAndEmptySnapshotsDropPreviousHits() {
        let session = sessions([sessionJSON()])[0]
        let cache = SearchIndexes()
        _ = cache.live([session], folders: [:])
        #expect(cache.live([], folders: [:]).search(session.title).isEmpty)
    }
}
