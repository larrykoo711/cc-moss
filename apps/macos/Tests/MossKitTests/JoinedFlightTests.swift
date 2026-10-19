import Foundation
import Testing
@testable import MossKit

@Suite struct JoinedFlightTests {
    @Test func requestsDuringARunShareOneMoreRun() async {
        let runs = Counter()
        let flight = JoinedFlight {
            runs.bump()
            try? await Task.sleep(nanoseconds: 100_000_000)
        }
        await withTaskGroup(of: Void.self) { group in
            for _ in 0..<10 { group.addTask { await flight.request() } }
        }
        #expect(runs.value <= 3, "ten requests, a couple of runs; got \(runs.value)")
        #expect(runs.value >= 1)
    }

    /// A request made while a run is under way waits for a run that started
    /// after it: what it asked for is never dropped.
    @Test func aRequestDuringARunSeesARunThatStartedAfterIt() async {
        let state = Box()
        let flight = JoinedFlight {
            let seen = state.value
            try? await Task.sleep(nanoseconds: 100_000_000)
            state.read = seen
        }
        let first = Task { await flight.request() }
        try? await Task.sleep(nanoseconds: 30_000_000)
        state.value = 1 // changes after the first run looked
        await flight.request()
        #expect(state.read == 1, "the second request was served by a run that saw the change")
        await first.value
    }

    @Test func aLoneRequestRunsOnce() async {
        let runs = Counter()
        let flight = JoinedFlight { runs.bump() }
        await flight.request()
        await flight.request()
        #expect(runs.value == 2)
    }
}

final class Box: @unchecked Sendable {
    private let lock = NSLock()
    private var _value = 0
    private var _read = -1
    var value: Int {
        get { lock.lock(); defer { lock.unlock() }; return _value }
        set { lock.lock(); _value = newValue; lock.unlock() }
    }
    var read: Int {
        get { lock.lock(); defer { lock.unlock() }; return _read }
        set { lock.lock(); _read = newValue; lock.unlock() }
    }
}
