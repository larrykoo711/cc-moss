import Foundation
import MossKit
import Observation

/// The app's one reader of `cc-moss events --json --since`: follows the
/// model's refreshes (no timer of its own) and hands new events to whoever
/// listens (notifications, Mo). On launch it only finds its place in the
/// stream; old events are never replayed.
@MainActor
final class EventFeed {
    private let model: AppModel
    private var cursor = EventCursor()
    private var fetching = false
    /// A refresh came in while fetching: fetch once more afterwards.
    private var again = false
    private var listeners: [([SessionEvent]) -> Void] = []

    init(model: AppModel) {
        self.model = model
    }

    func listen(_ body: @escaping ([SessionEvent]) -> Void) {
        listeners.append(body)
    }

    func start() {
        observe()
        fetch()
    }

    private func observe() {
        withObservationTracking {
            _ = model.sessions
        } onChange: { [weak self] in
            Task { @MainActor in
                guard let self else { return }
                self.fetch()
                self.observe()
            }
        }
    }

    private func fetch() {
        guard let client = model.client else { return }
        guard !fetching else {
            again = true
            return
        }
        fetching = true
        let since = cursor.since
        Task { [weak self] in
            let page = try? await client.events(since: since)
            guard let self else { return }
            self.fetching = false
            if let page {
                let fresh = self.cursor.advance(page)
                if !fresh.isEmpty { self.listeners.forEach { $0(fresh) } }
            }
            if self.again {
                self.again = false
                self.fetch()
            }
        }
    }
}
