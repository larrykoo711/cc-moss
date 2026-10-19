import Foundation
import Testing
@testable import MossKit

@Suite struct PanelOpenedTests {
    private func stateDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ccmoss-state-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    @Test func theFirstOpenMakesThePrivateMark() throws {
        let dir = try stateDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        try PanelOpenedMark.touch(stateDir: dir.path)
        let path = dir.appendingPathComponent("desktop/panel-opened").path
        let attrs = try FileManager.default.attributesOfItem(atPath: path)
        #expect((attrs[.posixPermissions] as? NSNumber)?.intValue == 0o600)
    }

    @Test func laterOpensMoveItsTime() throws {
        let dir = try stateDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let path = dir.appendingPathComponent("desktop/panel-opened").path
        try PanelOpenedMark.touch(stateDir: dir.path, now: Date(timeIntervalSince1970: 1_000_000))
        try PanelOpenedMark.touch(stateDir: dir.path, now: Date(timeIntervalSince1970: 2_000_000))
        let modified = try FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as? Date
        #expect(modified == Date(timeIntervalSince1970: 2_000_000))
    }

    @Test func mossCanPinTheCheckupAndTidy() {
        #expect(MossQuick.id(fromMoss: "checkup") == "action:checkup")
        #expect(MossQuick.id(fromMoss: "tidy") == "action:tidy")
    }
}
