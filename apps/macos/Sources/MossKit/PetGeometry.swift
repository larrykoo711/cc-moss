import CoreGraphics
import Foundation

/// Mo's face: the Continuum (x = 32 + 27 cos t, y = 32 + 12.9 sin 2t in a
/// 64-unit box, y down) with a few knobs. Every pose is one set of these.
public struct PetFace: Equatable, Sendable {
    public enum Part: Sendable {
        /// The whole loop, with its gap: the eyes are open.
        case loop
        /// The top half of each lobe: smiling arcs.
        case upper
        /// The bottom half of each lobe: shut eyes.
        case lower
    }

    public var part: Part = .loop
    /// Height of each lobe (12.9 is the icon's).
    public var amp = 12.9
    /// Vertical scale of the left and right lobes (blink, wide eyes).
    public var left = 1.0
    public var right = 1.0
    /// The outer ends move down by this much: a worried face.
    public var droop = 0.0
    /// The gap, as fractions of the loop's length; equal closes it.
    public var gapStart = 0.695
    public var gapEnd = 0.805

    public init(part: Part = .loop, amp: Double = 12.9, left: Double = 1, right: Double = 1,
                droop: Double = 0, gapStart: Double = 0.695, gapEnd: Double = 0.805) {
        self.part = part
        self.amp = amp
        self.left = left
        self.right = right
        self.droop = droop
        self.gapStart = gapStart
        self.gapEnd = gapEnd
    }

    /// The same face with both lobes squashed to `scale` (a blink).
    public func blinking(_ scale: Double) -> PetFace {
        var f = self
        f.left *= scale
        f.right *= scale
        return f
    }
}

/// Points for a face, always the same number whatever the pose, so Core
/// Animation can morph any pose into any other without in-between frames.
public struct PetShape: Equatable, Sendable {
    /// The loop drawn `PetGeometry.laps` times over: the gap is
    /// `strokeStart`/`strokeEnd` on it, and it can travel a whole lap without
    /// wrapping past the end of the path.
    public var loop: [CGPoint]
    /// Two arcs, left eye then right, for the smiling and sleeping faces.
    public var arcs: [[CGPoint]]
}

public enum PetGeometry {
    public static let box = 64.0
    public static let laps = 4
    public static let lapPoints = 160
    public static let arcPoints = 48

    /// A point of the loop at parameter `t` (not arc length).
    static func point(_ t: Double, _ face: PetFace) -> CGPoint {
        let c = cos(t)
        let scale = c < 0 ? face.left : face.right
        let x = 32 + 27 * c
        let y = 32 + face.amp * sin(2 * t) * scale + face.droop * pow(abs(c), 3)
        return CGPoint(x: x, y: y)
    }

    public static func shape(_ face: PetFace) -> PetShape {
        let total = laps * lapPoints
        let loop = (0...total).map { i in point(Double(i) / Double(lapPoints) * 2 * .pi, face) }
        return PetShape(loop: loop, arcs: arcs(face))
    }

    /// Smiling arcs (`upper`) or shut eyes (`lower`). A loop face still gets
    /// arcs (its upper ones), so every pose has both parts.
    static func arcs(_ face: PetFace) -> [[CGPoint]] {
        // sin 2t < 0 on (pi/2, pi) and (3pi/2, 2pi): the top of each lobe.
        // The 0.25 keeps the arcs apart where the lobes meet.
        let pi = Double.pi, trim = 0.25
        let lower: [(Double, Double)] = [(pi, 1.5 * pi - trim), (0, pi / 2 - trim)]
        let upper: [(Double, Double)] = [(pi / 2 + trim, pi), (1.5 * pi + trim, 2 * pi)]
        let ranges = face.part == .lower ? lower : upper
        return ranges.map { from, to in
            (0..<arcPoints).map { i in
                point(from + (to - from) * Double(i) / Double(arcPoints - 1), face)
            }
        }
    }

    /// Where the drawn stroke begins (the head of the travelling gap), as a
    /// fraction of the loop's length: the working spark sits here. Like the
    /// prototype's `trimmedPath` and Core Animation's `strokeStart`, the gap
    /// and the phase are fractions of length.
    public static func head(_ face: PetFace, phase: Double) -> Double {
        face.gapEnd + phase
    }

    /// `strokeStart` and `strokeEnd` on the loop path for a gap that has
    /// travelled `phase` (0...1) round the loop. One more lap adds
    /// `1 / laps` to both; the gap keeps its length all the way.
    public static func stroke(_ face: PetFace, phase: Double) -> (Double, Double) {
        let lap = 1 / Double(laps)
        let start = head(face, phase: phase) * lap
        return (start, start + (1 - (face.gapEnd - face.gapStart)) * lap)
    }

    /// Where a fraction of the loop's length sits.
    public static func point(atFraction f: Double, of face: PetFace) -> CGPoint {
        lap(face, from: f, count: 2)[0]
    }

    /// One lap starting at length fraction `from`, evenly spaced by length:
    /// the path the working spark rides.
    public static func lap(_ face: PetFace, from: Double, count: Int = lapPoints + 1) -> [CGPoint] {
        let one = (0...lapPoints).map { i in point(Double(i) / Double(lapPoints) * 2 * .pi, face) }
        var cumulative = [0.0]
        for i in 1..<one.count {
            cumulative.append(cumulative[i - 1] + hypot(one[i].x - one[i - 1].x, one[i].y - one[i - 1].y))
        }
        let length = cumulative.last ?? 1
        func at(_ fraction: Double) -> CGPoint {
            let d = (fraction - fraction.rounded(.down)) * length
            var i = 1
            while i < cumulative.count - 1, cumulative[i] < d { i += 1 }
            let span = cumulative[i] - cumulative[i - 1]
            let k = span > 0 ? (d - cumulative[i - 1]) / span : 0
            return CGPoint(x: one[i - 1].x + (one[i].x - one[i - 1].x) * k,
                           y: one[i - 1].y + (one[i].y - one[i - 1].y) * k)
        }
        return (0..<count).map { i in at(from + Double(i) / Double(max(count - 1, 1))) }
    }
}

/// The colours a look asks for; the app maps them to real colours.
public enum PetTint: Sendable {
    case brand, brandDim, amber, gray
}

/// Extras drawn around Mo in some poses.
public enum PetAccessory: Sendable {
    case none, sparkles, zzz, waves
}

/// Everything about how a pose looks, as numbers (DESIGN-pet §4). Lengths
/// are in units of Mo's size; angles in degrees, clockwise.
public struct PetLook: Equatable, Sendable {
    public var face: PetFace
    public var eyes: PetTint
    public var lamp: PetTint
    public var antennaTilt: Double
    /// Body squash, anchored at the feet (worry).
    public var squashX = 1.0
    public var squashY = 1.0
    /// How high a hop goes, in units of size; 0 for poses that do not hop.
    public var hop = 0.0
    public var hops = 0
    public var accessory: PetAccessory = .none
    public var eyeOpacity = 1.0
    public var glow = true
    /// The whole of Mo dims a little while asleep.
    public var dim = 1.0
    /// A white spark rides the head of the moving gap (working).
    public var spark = false

    public static func of(_ pose: PetPose) -> PetLook {
        switch pose {
        case .idle:
            PetLook(face: PetFace(), eyes: .brand, lamp: .brandDim, antennaTilt: 0)
        case .working:
            PetLook(face: PetFace(), eyes: .brand, lamp: .brand, antennaTilt: 0, spark: true)
        case .needsYou:
            PetLook(face: PetFace(amp: 15.5, left: 1.1, right: 1.1, gapStart: 0.72, gapEnd: 0.78),
                    eyes: .brand, lamp: .brand, antennaTilt: 0, hop: 0.10, hops: 2, accessory: .waves)
        case .interrupted:
            PetLook(face: PetFace(amp: 11, droop: 5, gapStart: 0.60, gapEnd: 0.90),
                    eyes: .amber, lamp: .amber, antennaTilt: -32, squashX: 1.03, squashY: 0.95)
        case .happy:
            PetLook(face: PetFace(part: .upper, amp: 16), eyes: .brand, lamp: .brand, antennaTilt: 0,
                    hop: 0.07, hops: 1, accessory: .sparkles)
        case .sleeping:
            PetLook(face: PetFace(part: .lower, amp: 6), eyes: .brand, lamp: .gray, antennaTilt: -14,
                    accessory: .zzz, eyeOpacity: 0.55, glow: false, dim: 0.82)
        }
    }
}
