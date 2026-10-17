import Testing
@testable import MossKit

@Suite struct VersionMatchTests {
    @Test func whichOneIsBehind() {
        #expect(VersionMatch.of(app: "0.2.0", cli: "0.2.0") == .same)
        #expect(VersionMatch.of(app: "0.2.0", cli: "0.1.1") == .cliOlder)
        #expect(VersionMatch.of(app: "0.1.1", cli: "0.2.0") == .appOlder)
        #expect(VersionMatch.of(app: "0.10.0", cli: "0.9.9") == .cliOlder, "numbers, not text")
    }

    @Test func preReleaseIsBeforeItsRelease() {
        #expect(VersionMatch.of(app: "0.2.0-rc.1", cli: "0.2.0") == .appOlder)
        #expect(VersionMatch.of(app: "0.2.0-rc.2", cli: "0.2.0-rc.1") == .cliOlder)
    }

    @Test func devBuildsSuggestNothing() {
        #expect(VersionMatch.of(app: "dev", cli: "0.2.0") == .unknown)
        #expect(VersionMatch.of(app: "0.2.0", cli: "—") == .unknown)
    }
}
