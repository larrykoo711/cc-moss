import CoreGraphics
import Foundation
import Testing
@testable import MossKit

@Suite struct PetGeometryTests {
    @Test func samePointCountForAllPoses() {
        let shapes = PetPose.allCases.map { PetGeometry.shape(PetLook.of($0).face) }
        let loops = Set(shapes.map(\.loop.count))
        let arcs = Set(shapes.map { $0.arcs.map(\.count) })
        #expect(loops.count == 1)
        #expect(arcs.count == 1)
        #expect(loops.first == PetGeometry.laps * PetGeometry.lapPoints + 1)
    }

    @Test func blinkKeepsThePointCount() {
        let open = PetGeometry.shape(PetLook.of(.idle).face)
        let shut = PetGeometry.shape(PetLook.of(.idle).face.blinking(0.1))
        #expect(open.loop.count == shut.loop.count)
    }

    @Test func upperArcForHappy() {
        let happy = PetLook.of(.happy).face
        #expect(happy.part == .upper)
        let points = PetGeometry.shape(happy).arcs.flatMap { $0 }
        // y grows downwards: the upper half is at or above the middle line.
        #expect(points.allSatisfy { $0.y <= PetGeometry.box / 2 + 1e-9 })
        #expect(points.contains { $0.y < PetGeometry.box / 2 - 10 })
    }

    @Test func lowerArcForSleeping() {
        let sleeping = PetLook.of(.sleeping).face
        #expect(sleeping.part == .lower)
        let points = PetGeometry.shape(sleeping).arcs.flatMap { $0 }
        #expect(points.allSatisfy { $0.y >= PetGeometry.box / 2 - 1e-9 })
    }

    @Test func eachArcStaysOnItsOwnEye() {
        let arcs = PetGeometry.shape(PetLook.of(.happy).face).arcs
        #expect(arcs.count == 2)
        #expect(arcs[0].allSatisfy { $0.x < PetGeometry.box / 2 })
        #expect(arcs[1].allSatisfy { $0.x > PetGeometry.box / 2 })
    }

    @Test func parametersStayInRange() {
        for pose in PetPose.allCases {
            let face = PetLook.of(pose).face
            #expect(face.left > 0 && face.right > 0, "\(pose)")
            #expect((0...1).contains(face.gapStart) && (0...1).contains(face.gapEnd) && face.gapStart <= face.gapEnd, "\(pose)")
            for phase in [0.0, 0.25, 0.5, 0.999, 1.0] {
                let (start, end) = PetGeometry.stroke(face, phase: phase)
                #expect(start >= 0 && end <= 1 && start < end, "\(pose) \(phase)")
            }
            let shape = PetGeometry.shape(face)
            for p in shape.loop + shape.arcs.flatMap({ $0 }) {
                #expect((0...PetGeometry.box).contains(p.x) && (0...PetGeometry.box).contains(p.y), "\(pose)")
            }
        }
    }

    @Test func gapLengthIsTheSameAtEveryPhase() {
        let face = PetLook.of(.working).face
        let lengths = [0.0, 0.3, 0.7, 1.0].map { phase -> Double in
            let (s, e) = PetGeometry.stroke(face, phase: phase)
            return e - s
        }
        #expect(lengths.allSatisfy { abs($0 - lengths[0]) < 1e-12 })
    }

    @Test func aFullLapLooksLikeTheStart() {
        // The flow animation adds one lap: the picture must not jump at the seam.
        let face = PetLook.of(.working).face
        let a = PetGeometry.stroke(face, phase: 0)
        let b = PetGeometry.stroke(face, phase: 1)
        #expect(abs((b.0 - a.0) - 1 / Double(PetGeometry.laps)) < 1e-12)
        #expect(abs((b.1 - a.1) - 1 / Double(PetGeometry.laps)) < 1e-12)
    }

    @Test func sparkLapStartsAtTheGapEnd() {
        let face = PetLook.of(.working).face
        let lap = PetGeometry.lap(face, from: PetGeometry.head(face, phase: 0))
        let expected = PetGeometry.point(atFraction: face.gapEnd, of: face)
        #expect(abs(lap[0].x - expected.x) < 0.01 && abs(lap[0].y - expected.y) < 0.01)
        #expect(abs(lap[0].x - lap[lap.count - 1].x) < 0.01 && abs(lap[0].y - lap[lap.count - 1].y) < 0.01)
    }

    @Test func idleLoopMatchesTheIcon() {
        // x = 32 + 27 cos t, y = 32 + 12.9 sin 2t at t = 0 and t = pi/4.
        let loop = PetGeometry.shape(PetLook.of(.idle).face).loop
        #expect(abs(loop[0].x - 59) < 1e-9 && abs(loop[0].y - 32) < 1e-9)
        let eighth = loop[PetGeometry.lapPoints / 8]
        #expect(abs(eighth.x - (32 + 27 * cos(Double.pi / 4))) < 1e-9)
        #expect(abs(eighth.y - (32 + 12.9)) < 1e-9)
    }
}
