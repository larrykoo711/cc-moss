import CoreGraphics

/// Keeps hover from fighting a scroll. While the list scrolls, rows slide
/// under a still pointer: those aren't hovers, and showing each one's
/// buttons redraws the list on every row. After a scroll, hover counts again
/// once the pointer itself moves. A reference type on purpose: noting a scroll must
/// not redraw anything.
public final class HoverGate {
    /// Where the pointer was at the last scroll; nil once it has moved since.
    private var stillAt: CGPoint?

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
}
