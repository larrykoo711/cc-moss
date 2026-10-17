import Foundation

/// How folders read in a picker, and folders typed as a path.
public enum FolderChoice {
    public struct Label: Equatable, Sendable {
        public let name: String
        /// Where it lives, `~`-shortened; nil for home and the root.
        public let parent: String?

        public init(name: String, parent: String?) {
            self.name = name
            self.parent = parent
        }
    }

    /// "emma-studio" in "~/Documents": the name first, so long paths stay
    /// readable on one line.
    public static func label(_ path: String, home: String) -> Label {
        if path == home { return Label(name: "~", parent: nil) }
        let url = URL(fileURLWithPath: path)
        let name = url.lastPathComponent
        guard path != "/", !name.isEmpty else { return Label(name: "/", parent: nil) }
        let parent = url.deletingLastPathComponent().path
        return Label(name: name, parent: Grouping.tilde(parent, home: home))
    }

    /// The query, when it's a path (starts with `/` or `~`) to a folder that
    /// exists; otherwise it's just a search.
    public static func typedPath(_ query: String, home: String, isFolder: (String) -> Bool) -> String? {
        var text = query.trimmingCharacters(in: .whitespaces)
        guard text.hasPrefix("/") || text.hasPrefix("~") else { return nil }
        if text == "~" { text = home } else if text.hasPrefix("~/") { text = home + text.dropFirst(1) }
        guard text.hasPrefix("/") else { return nil }
        let path = URL(fileURLWithPath: text).standardizedFileURL.path
        return isFolder(path) ? path : nil
    }
}
