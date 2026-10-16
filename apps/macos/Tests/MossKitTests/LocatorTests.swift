import Foundation
import Testing
@testable import MossKit

@Suite struct LocatorTests {
    @Test func searchOrder() {
        let l = Locator(home: "/home/u", override: "/opt/x/cc-moss", isExecutable: { _ in false })
        #expect(l.candidates == [
            "/opt/x/cc-moss", "/opt/homebrew/bin/cc-moss", "/usr/local/bin/cc-moss",
            "/home/u/.cargo/bin/cc-moss", "/home/u/.local/bin/cc-moss",
        ])
        #expect(l.locate() == nil)
    }

    @Test func firstExecutableWins() {
        let l = Locator(home: "/home/u", isExecutable: { $0.hasPrefix("/usr/local") || $0.contains(".cargo") })
        #expect(l.locate() == "/usr/local/bin/cc-moss")
    }
}
