import Foundation

/// `cc-moss://panel` links: open the panel, on a page if one is named
/// (`cc-moss://panel/moss`, `/queue`, `/sessions`). Anything else isn't ours.
public enum PanelLink: Equatable, Sendable {
    case panel
    case sessions
    case queue
    case moss

    public static let scheme = "cc-moss"

    public init?(_ url: URL) {
        guard url.scheme?.lowercased() == Self.scheme, url.host?.lowercased() == "panel" else { return nil }
        switch url.path.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "/")) {
        case "": self = .panel
        case "sessions": self = .sessions
        case "queue", "todo": self = .queue
        case "moss": self = .moss
        default: return nil
        }
    }
}
