import CoreGraphics

/// Keeps hover from fighting a scroll. While the list scrolls, rows slide
/// under a still pointer: those aren't hovers, and acting on each one
/// (selecting it, which scrolls to it) redraws the list on every row and
/// pulls against the scroll. After a scroll, hover counts again once the
/// pointer itself moves. A reference type on purpose: noting a scroll must
/// not redraw anything.
public final class HoverGate {
    /// Where the pointer was at the last scroll; nil once it has moved since.
    private var stillAt: CGPoint?
    private var hoverSelected = false

    public init() {}

    public func scrolled(pointer: CGPoint) {
        stillAt = pointer
    }

    public func allowsHover(pointer: CGPoint) -> Bool {
        guard let stillAt else { return true }
        if pointer == stillAt { return false }
        self.stillAt = nil
        return true
    }

    /// The next selection change comes from the pointer.
    public func selectingByHover() {
        hoverSelected = true
    }

    /// Scroll to a new selection only when the keyboard (or the app) made it:
    /// a hovered row is already in view.
    public func shouldScrollToSelection() -> Bool {
        defer { hoverSelected = false }
        return !hoverSelected
    }
}
