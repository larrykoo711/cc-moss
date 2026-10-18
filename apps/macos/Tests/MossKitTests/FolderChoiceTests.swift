import Testing
@testable import MossKit

@Suite struct FolderChoiceTests {
    let home = "/Users/demo"

    @Test func splitsNameFromWhereItLives() {
        #expect(FolderChoice.label("/Users/demo/Documents/emma-studio", home: home) == .init(name: "emma-studio", parent: "~/Documents"))
        #expect(FolderChoice.label("/Users/demo/code", home: home) == .init(name: "code", parent: "~"))
        #expect(FolderChoice.label("/Users/demo", home: home) == .init(name: "~", parent: nil))
        #expect(FolderChoice.label("/opt/work", home: home) == .init(name: "work", parent: "/opt"))
        #expect(FolderChoice.label("/", home: home) == .init(name: "/", parent: nil))
    }

    @Test func aTypedPathCountsWhenItIsAFolder() {
        let folders: Set<String> = ["/Users/demo/code", "/tmp"]
        let exists: (String) -> Bool = { folders.contains($0) }
        #expect(FolderChoice.typedPath("~/code", home: home, isFolder: exists) == "/Users/demo/code")
        #expect(FolderChoice.typedPath(" ~/code/ ", home: home, isFolder: exists) == "/Users/demo/code")
        #expect(FolderChoice.typedPath("/tmp", home: home, isFolder: exists) == "/tmp")
        #expect(FolderChoice.typedPath("~", home: home, isFolder: { $0 == "/Users/demo" }) == "/Users/demo")
    }

    @Test func searchWordsAreNotPaths() {
        let exists: (String) -> Bool = { _ in true }
        #expect(FolderChoice.typedPath("emma", home: home, isFolder: exists) == nil)
        #expect(FolderChoice.typedPath("", home: home, isFolder: exists) == nil)
        #expect(FolderChoice.typedPath("~/missing", home: home, isFolder: { _ in false }) == nil)
    }
}
