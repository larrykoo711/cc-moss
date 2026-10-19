import Foundation

/// Where the sessions panel goes (screen coordinates, y up): under the menu
/// bar icon, left edges lined up, inside the screen's visible area. Right
/// after a cold launch the icon may not be laid out yet (an empty frame, or
/// one at the bottom left, not in a menu bar): then the top right of the
/// screen under the pointer.
public enum PanelPlacement {
    public struct Screen: Equatable, Sendable {
        public var frame: CGRect
        public var visible: CGRect

        public init(frame: CGRect, visible: CGRect) {
            self.frame = frame
            self.visible = visible
        }
    }

    public static let margin: CGFloat = 8
    /// Between the icon and the panel's top.
    public static let gap: CGFloat = 5

    /// - screens: the main screen first.
    public static func frame(anchor: CGRect?, screens: [Screen], mouse: CGPoint, size: CGSize) -> CGRect {
        // A laid-out icon sits in a screen's menu bar, at its top (the bar may
        // hide itself, so not "above the visible area"); a not-yet-placed one
        // reads as an empty or bottom-left rectangle.
        let usable = anchor.flatMap { a in
            a.width > 0 && a.height > 0 && screens.contains { $0.frame.intersects(a) && a.midY > $0.frame.maxY - 50 } ? a : nil
        }
        let screen = usable.flatMap { a in screens.first { $0.frame.intersects(a) } }
            ?? screens.first { $0.frame.contains(mouse) }
            ?? screens.first
        guard let visible = screen?.visible else {
            return CGRect(origin: .zero, size: size)
        }
        let top = usable.map { $0.minY - gap } ?? (visible.maxY - margin)
        var x = usable?.minX ?? (visible.maxX - size.width - margin)
        // Always inside the visible area.
        x = min(max(x, visible.minX + margin), visible.maxX - size.width - margin)
        var y = top - size.height
        y = min(max(y, visible.minY + margin), visible.maxY - size.height)
        return CGRect(x: x, y: y, width: size.width, height: size.height)
    }
}
