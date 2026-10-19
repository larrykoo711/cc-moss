import Foundation

/// How an answer's label reads: Claude marks the one it suggests with
/// "(Recommended)" at the end. Shown as a small tag instead; the label sent
/// back is always the session's own, unchanged.
public enum OptionLabel {
    static let marks = ["(recommended)", "（recommended）", "(推荐)", "（推荐）"]

    /// The label without the mark, and whether it had one.
    public static func split(_ label: String) -> (title: String, recommended: Bool) {
        let trimmed = label.trimmingCharacters(in: .whitespaces)
        let lower = trimmed.lowercased()
        for mark in marks where lower.hasSuffix(mark) {
            let title = String(trimmed.dropLast(mark.count)).trimmingCharacters(in: .whitespaces)
            guard !title.isEmpty else { break }
            return (title, true)
        }
        return (label, false)
    }
}
