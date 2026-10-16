import CoreGraphics

/// Where Mo's puff or message box goes on screen (DESIGN-pet §6.3): above
/// Mo and to its left by default, the tail's tip on Mo's head; to the right
/// when there is no room on the left, below Mo when there is none above,
/// and always inside the screen's visible area.
///
/// Screen coordinates, y up. The window is the bubble plus `margin` on
/// every side, room for its shadow, so the shadow never meets the window's
/// edge (a shadow cut off there shows as a rectangle).
public struct BubblePlacement: Equatable, Sendable {
    /// The bubble's window.
    public var frame: CGRect
    /// The tail's tip, from the bubble's leading edge (not the window's).
    public var tailX: CGFloat
    /// True: the bubble is left of Mo, so the tail leans right toward it.
    public var leansRight: Bool
    /// True: the bubble is below Mo, its tail on the top edge.
    public var tailOnTop: Bool

    /// Kept between the bubble and the screen's edges.
    public static let edge: CGFloat = 8

    /// - size: the window (bubble plus margins).
    /// - inset: how far from the bubble's near side the tail sits.
    /// - above: the point the tail touches when the bubble is above Mo (its head).
    /// - below: the point it touches when the bubble is below Mo (its feet).
    public static func place(size: CGSize, margin: CGFloat, inset: CGFloat, corner: CGFloat,
                             above: CGPoint, below: CGPoint, visible: CGRect) -> BubblePlacement {
        let width = size.width - 2 * margin
        // Left of Mo first; to the right if the left side runs off the screen.
        var tailX = width - inset
        var leansRight = true
        if above.x - tailX < visible.minX + edge {
            tailX = inset
            leansRight = false
        }
        var x = above.x - margin - tailX
        x = min(max(x, visible.minX + edge - margin), visible.maxX - edge - margin - width)

        // Above Mo, tip on its head; below when the top would leave the screen.
        var y = above.y - margin
        var onTop = false
        if y + size.height - margin > visible.maxY - edge {
            onTop = true
            y = below.y + margin - size.height
        }
        y = max(y, visible.minY + edge - margin)

        // After clamping, the tail still points at Mo, but never into a corner.
        let lowest = corner + 8, highest = width - corner - 8
        tailX = min(max(above.x - x - margin, lowest), max(lowest, highest))
        return BubblePlacement(frame: CGRect(x: x, y: y, width: size.width, height: size.height),
                               tailX: tailX, leansRight: leansRight, tailOnTop: onTop)
    }
}
