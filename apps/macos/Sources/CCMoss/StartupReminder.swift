import Foundation
import MossKit
import Observation

/// Remembers which sessions are running, and on the first list after launch
/// tells you about those that stopped while the app was away (A8).
@MainActor
final class StartupReminder {
    private let model: AppModel
    private let defaults: UserDefaults
    private let remind: (StartupCheck.Reminder) -> Void
    private var checked = false

    init(model: AppModel, defaults: UserDefaults = .standard, remind: @escaping (StartupCheck.Reminder) -> Void) {
        self.model = model
        self.defaults = defaults
        self.remind = remind
    }

    func start() {
        withObservationTracking {
            _ = model.sessions
            _ = model.phase
        } onChange: { [weak self] in
            Task { @MainActor in
                guard let self else { return }
                self.update()
                self.start()
            }
        }
        update()
    }

    private func update() {
        guard model.phase == .ready else { return }
        if !checked {
            checked = true
            remind(StartupCheck.reminder(model.sessions, lastRunning: StartupCheck.load(defaults)))
        }
        let running = StartupCheck.running(model.sessions)
        if StartupCheck.load(defaults) != running { StartupCheck.save(running, defaults) }
    }
}
