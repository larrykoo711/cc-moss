import CoreGraphics
import Foundation

/// A display as the pet sees it, in AppKit screen coordinates (y up).
public struct PetScreen: Equatable, Sendable {
    /// Stable across launches (the display's UUID).
    public var id: String
    public var frame: CGRect
    /// Without the menu bar and the Dock.
    public var visible: CGRect
    public var isMain: Bool

    public init(id: String, frame: CGRect, visible: CGRect, isMain: Bool) {
        self.id = id
        self.frame = frame
        self.visible = visible
        self.isMain = isMain
    }
}

/// Where Mo sits: one spot per screen, kept relative to that screen's visible
/// area so a resolution change or a moved Dock keeps it in the same corner.
public struct PetPlacement: Codable, Equatable, Sendable {
    /// A spot as fractions of the room left in the visible area (0 = left /
    /// bottom edge, 1 = right / top edge).
    public struct Spot: Codable, Equatable, Sendable {
        public var x: Double
        public var y: Double
    }

    public static let snapDistance: CGFloat = 24
    /// The default spot: bottom right, this far from the edge and the Dock.
    public static let margin: CGFloat = 24

    public var spots: [String: Spot] = [:]
    public var lastScreen: String?

    public init() {}

    /// Remember a frame the user left Mo at.
    public mutating func remember(_ frame: CGRect, on screen: PetScreen) {
        let v = screen.visible
        let roomX = v.width - frame.width, roomY = v.height - frame.height
        let x = roomX > 0 ? Double((frame.minX - v.minX) / roomX) : 0
        let y = roomY > 0 ? Double((frame.minY - v.minY) / roomY) : 0
        spots[screen.id] = Spot(x: min(max(x, 0), 1), y: min(max(y, 0), 1))
        lastScreen = screen.id
    }

    /// Where a window of `size` goes now: the last screen's spot if that
    /// screen is still here, else the main screen's bottom right corner.
    public func frame(size: CGSize, screens: [PetScreen]) -> CGRect {
        if let id = lastScreen, let screen = screens.first(where: { $0.id == id }), let spot = spots[id] {
            let v = screen.visible
            let origin = CGPoint(x: (v.minX + CGFloat(spot.x) * (v.width - size.width)).rounded(),
                                 y: (v.minY + CGFloat(spot.y) * (v.height - size.height)).rounded())
            return Self.clamp(CGRect(origin: origin, size: size), into: v)
        }
        guard let main = screens.first(where: \.isMain) ?? screens.first else {
            return CGRect(origin: .zero, size: size)
        }
        return Self.defaultFrame(size: size, on: main)
    }

    public static func defaultFrame(size: CGSize, on screen: PetScreen) -> CGRect {
        let v = screen.visible
        let f = CGRect(x: v.maxX - margin - size.width, y: v.minY + margin, width: size.width, height: size.height)
        return clamp(f, into: v)
    }

    /// Moved back inside the visible area, by as little as possible.
    public static func clamp(_ frame: CGRect, into visible: CGRect) -> CGRect {
        var f = frame
        f.origin.x = min(max(f.minX, visible.minX), max(visible.maxX - f.width, visible.minX))
        f.origin.y = min(max(f.minY, visible.minY), max(visible.maxY - f.height, visible.minY))
        return f
    }

    /// Let go within `snapDistance` of an edge and it moves flush to it.
    public static func snap(_ frame: CGRect, in visible: CGRect) -> CGRect {
        var f = clamp(frame, into: visible)
        if f.minX - visible.minX <= snapDistance { f.origin.x = visible.minX }
        if visible.maxX - f.maxX <= snapDistance { f.origin.x = visible.maxX - f.width }
        if f.minY - visible.minY <= snapDistance { f.origin.y = visible.minY }
        if visible.maxY - f.maxY <= snapDistance { f.origin.y = visible.maxY - f.height }
        return f
    }

    /// "Tuck away": half of Mo goes past the nearer side edge.
    public static func tucked(_ frame: CGRect, in visible: CGRect) -> CGRect {
        var f = clamp(frame, into: visible)
        let toRight = visible.maxX - f.midX < f.midX - visible.minX
        f.origin.x = (toRight ? visible.maxX : visible.minX) - f.width / 2
        return f
    }

    /// The screen most of a frame is on.
    public static func screen(for frame: CGRect, among screens: [PetScreen]) -> PetScreen? {
        func overlap(_ s: PetScreen) -> CGFloat {
            let i = s.frame.intersection(frame)
            return i.isNull ? 0 : i.width * i.height
        }
        let best = screens.max { overlap($0) < overlap($1) }
        guard let best, overlap(best) > 0 else { return screens.first(where: \.isMain) ?? screens.first }
        return best
    }
}
