import Foundation
import Testing

@Suite struct ScratchDefaultsTests {
    @Test func writesStayInTheTemporaryFolder() throws {
        let label = "ScratchDefaultsTests-\(UUID().uuidString)"
        let defaults = scratchDefaults(label)
        defaults.set("kept", forKey: "probe")
        defaults.synchronize()
        #expect(scratchDefaults(label).string(forKey: "probe") == nil, "each call is a fresh suite")
        #expect(defaults.string(forKey: "probe") == "kept")
        let written = try FileManager.default.contentsOfDirectory(atPath: ScratchDefaults.root.path)
        #expect(written.contains { $0.hasPrefix(label) })
        let library = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Preferences")
        let leaked = (try? FileManager.default.contentsOfDirectory(atPath: library.path)) ?? []
        #expect(!leaked.contains { $0.hasPrefix(label) })
    }

    /// A test that names a suite itself writes into `~/Library/Preferences` again.
    @Test func noTestNamesASuiteItself() throws {
        let folder = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        let files = try FileManager.default.contentsOfDirectory(atPath: folder.path)
            .filter { $0.hasSuffix(".swift") && $0 != "ScratchDefaults.swift" }
        let pattern = "UserDefaults(" + "suiteName:"
        let offenders = try files.filter {
            try String(contentsOf: folder.appendingPathComponent($0), encoding: .utf8).contains(pattern)
        }
        #expect(files.count > 10, "the test folder was found")
        #expect(offenders.isEmpty, "use scratchDefaults(_:) in \(offenders)")
    }
}
