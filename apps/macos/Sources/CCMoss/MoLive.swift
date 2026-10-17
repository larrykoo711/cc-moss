import AppKit
import MossKit
import SwiftUI

/// What Mo does once when `beat` changes.
enum MoMove: Equatable {
    case none, wave, hop, blink
}

/// The real Mo, alive, for the welcome and Settings: its own layers and
/// loops (breathing, blinking, the zzz), eyes that follow the pointer, and
/// a move played whenever `beat` changes.
struct MoLive: NSViewRepresentable {
    var pose: PetPose
    var body: CGFloat = 92
    var move: MoMove = .none
    /// Bump to play `move` again.
    var beat = 0

    func makeNSView(context: Context) -> PetView {
        let view = PetView(body: body)
        context.coordinator.attach(view)
        context.coordinator.update(pose: pose, move: move, beat: beat, first: true)
        return view
    }

    func updateNSView(_ view: PetView, context: Context) {
        context.coordinator.update(pose: pose, move: move, beat: beat, first: false)
    }

    static func dismantleNSView(_ view: PetView, coordinator: Coordinator) {
        coordinator.stop()
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: PetView, context: Context) -> CGSize? {
        nsView.frame.size
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    @MainActor
    final class Coordinator {
        private weak var view: PetView?
        private var pose: PetPose?
        private var beat = -1
        private var timer: Timer?
        private var nextBlink = Date().addingTimeInterval(3)

        func attach(_ view: PetView) {
            self.view = view
            // Eyes after the pointer, and a blink now and then: 20 times a second is plenty.
            timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.tick() }
            }
        }

        func update(pose: PetPose, move: MoMove, beat: Int, first: Bool) {
            guard let view else { return }
            if pose != self.pose {
                view.apply(PetLook.of(pose), animated: !first)
                view.setLoops(Self.loops(pose))
                self.pose = pose
            }
            if beat != self.beat {
                self.beat = beat
                if !first {
                    switch move {
                    case .wave: view.wave()
                    case .hop: view.hop()
                    case .blink: view.blink()
                    case .none: break
                    }
                }
            }
        }

        static func loops(_ pose: PetPose) -> Set<PetLoop> {
            if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion { return [] }
            switch pose {
            case .idle, .happy, .needsYou: return [.breathe]
            case .working: return [.flow]
            case .sleeping: return [.zzz]
            case .interrupted: return []
            }
        }

        private func tick() {
            guard let view, let window = view.window else { return }
            let p = view.convert(window.convertPoint(fromScreen: NSEvent.mouseLocation), from: nil)
            view.look(toward: pose == .sleeping ? nil : p)
            if Date() > nextBlink, pose != .sleeping {
                view.blink()
                nextBlink = Date().addingTimeInterval(.random(in: 3...6))
            }
        }

        func stop() {
            timer?.invalidate()
            timer = nil
        }
    }
}
