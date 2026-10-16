import AppKit
import MossKit
import QuartzCore
import SwiftUI

/// The animated mark, as Core Animation layers over the status button: the
/// render server plays the loop, so the app does no work per frame (swapping
/// the button's image every frame cost several percent CPU in system updates).
final class IconMotionView: NSView {
    private(set) var playing: IconState?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = false
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// Clicks go to the status button underneath.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func play(_ state: IconState) {
        build(state)
        let period = IconMotion.period(state)
        switch state {
        case .working:
            for (i, piece) in comets.enumerated() {
                let (start, end) = Self.cometSpan(piece: i, head: 0)
                let group = CAAnimationGroup()
                group.animations = [
                    Self.linear("strokeStart", from: start, to: start + 0.5),
                    Self.linear("strokeEnd", from: end, to: end + 0.5),
                ]
                group.duration = period
                group.repeatCount = .infinity
                group.timingFunction = CAMediaTimingFunction(name: .linear)
                piece.add(group, forKey: "flow")
            }
        case .needsYou:
            let pulse = CABasicAnimation(keyPath: "transform.scale")
            pulse.fromValue = 1.0
            pulse.toValue = 1.08
            pulse.duration = period / 2
            pulse.autoreverses = true
            pulse.repeatCount = .infinity
            pulse.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            mark?.add(pulse, forKey: "pulse")
        default:
            break
        }
        playing = state
        isHidden = false
    }

    func stop() {
        layer?.sublayers?.forEach { $0.removeFromSuperlayer() }
        comets = []
        mark = nil
        playing = nil
        isHidden = true
    }

    /// One still moment of the loop as plain layer values, no animation:
    /// for checking the layers off-screen (`--render --icon … --layers`).
    func pose(_ frame: IconFrame) {
        build(frame.state)
        switch frame.state {
        case .working:
            for (i, piece) in comets.enumerated() {
                let (start, end) = Self.cometSpan(piece: i, head: frame.progress)
                piece.strokeStart = start
                piece.strokeEnd = end
            }
        case .needsYou:
            let s = 1 + 0.04 * (1 - cos(2 * .pi * frame.progress))
            mark?.transform = CATransform3DMakeScale(s, s, 1)
        default:
            break
        }
        playing = frame.state
        isHidden = false
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        recolour()
    }

    // MARK: Layers

    private var mark: CAShapeLayer?
    private var comets: [CAShapeLayer] = []
    private var state: IconState?

    /// The bar's text colour in this appearance: what template images get.
    private var ink: CGColor {
        var colour = NSColor.black.cgColor
        effectiveAppearance.performAsCurrentDrawingAppearance {
            colour = NSColor.labelColor.withAlphaComponent(1).cgColor
        }
        return colour
    }

    private func build(_ state: IconState) {
        stop()
        guard let root = layer else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        self.state = state
        let box = CGRect(x: 0, y: 0, width: StatusIcon.side, height: StatusIcon.side)
        let base = Self.shape(Self.flipped(Continuum().path(in: box).cgPath))
        base.frame = box
        root.addSublayer(base)
        mark = base
        if state == .working {
            let lap = Continuum.loop(in: box).cgPath
            let twice = CGMutablePath()
            twice.addPath(lap)
            twice.addPath(lap)
            let path = Self.flipped(twice)
            for _ in 0..<StatusIcon.pieces {
                let piece = Self.shape(path)
                piece.frame = box
                root.addSublayer(piece)
                comets.append(piece)
            }
        }
        recolour()
    }

    private func recolour() {
        guard let state else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        switch state {
        case .needsYou:
            mark?.strokeColor = StatusIcon.brand.cgColor
        default:
            let ink = ink
            mark?.strokeColor = state == .working ? ink.copy(alpha: StatusIcon.dimmed) : ink
            for (i, piece) in comets.enumerated() {
                piece.strokeColor = ink.copy(alpha: CGFloat(i + 1) / CGFloat(comets.count))
            }
        }
    }

    /// Piece `piece` of the travelling spot as stroke fractions of the
    /// two-lap path, for the head at `head` (0..<1 of one lap). One lap is
    /// half the path, so adding 0.5 to both ends moves the spot once round.
    static func cometSpan(piece: Int, head: Double) -> (CGFloat, CGFloat) {
        let n = Double(StatusIcon.pieces)
        let from = head - StatusIcon.tail + StatusIcon.tail * Double(piece) / n
        let to = head - StatusIcon.tail + StatusIcon.tail * Double(piece + 1) / n
        // Start on the first lap even when the tail reaches back past 0.
        let lap = from < 0 ? 1.0 : 0.0
        return (CGFloat((from + lap) / 2), CGFloat((to + lap) / 2))
    }

    private static func shape(_ path: CGPath) -> CAShapeLayer {
        let l = CAShapeLayer()
        l.path = path
        l.fillColor = nil
        l.lineWidth = StatusIcon.lineWidth
        l.lineCap = .round
        l.lineJoin = .round
        l.actions = ["strokeStart": NSNull(), "strokeEnd": NSNull(), "transform": NSNull(), "strokeColor": NSNull()]
        return l
    }

    /// Continuum is drawn y-down; layers here are y-up.
    private static func flipped(_ path: CGPath) -> CGPath {
        var t = CGAffineTransform(translationX: 0, y: StatusIcon.side).scaledBy(x: 1, y: -1)
        return path.copy(using: &t) ?? path
    }

    private static func linear(_ key: String, from: CGFloat, to: CGFloat) -> CABasicAnimation {
        let a = CABasicAnimation(keyPath: key)
        a.fromValue = from
        a.toValue = to
        return a
    }
}

/// Forwards the animator's start/stop to the main actor's owner.
private final class MotionRelay: MotionDriver {
    var start: ((IconState) -> Void)?
    var stop: (() -> Void)?
    func startMotion(_ state: IconState) { start?(state) }
    func stopMotion() { stop?() }
}

/// The status item's content: a picture for the button and, while a state
/// animates, which animation the motion layers should play. Deliberately
/// not observable: nothing here goes through SwiftUI.
@MainActor
final class MenuBarIcon {
    /// The button's image: the whole icon when still; while moving, only
    /// the number (the layers draw the mark).
    private(set) var image = NSImage()
    private(set) var state: IconState = .idle
    private(set) var moving: IconState?
    /// Called whenever the image or the motion changes.
    var onChange: (() -> Void)?

    private let relay = MotionRelay()
    private let animator: IconAnimator
    private var badge: Int?
    private var observers: [NSObjectProtocol] = []
    private var screensOff = false
    private var locked = false
    /// Pictures are drawn once and reused.
    private var cache: [CacheKey: NSImage] = [:]

    private struct CacheKey: Hashable {
        let state: IconState
        let badge: Int?
        let mark: Bool
    }

    init() {
        animator = IconAnimator(driver: relay)
        relay.start = { [weak self] state in MainActor.assumeIsolated { self?.moving = state } }
        relay.stop = { [weak self] in MainActor.assumeIsolated { self?.moving = nil } }
        animator.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        publish()
        watchSystem()
    }

    /// New sessions from a refresh; `showCount` is the "Icon and number" setting.
    func update(_ sessions: [Session], showCount: Bool) {
        badge = showCount ? IconBadge.count(sessions) : nil
        state = IconState(sessions)
        animator.state = state
        publish()
    }

    private func publish() {
        let key = CacheKey(state: state, badge: badge, mark: moving == nil)
        let picture: NSImage
        if let hit = cache[key] {
            picture = hit
        } else {
            picture = StatusIcon.bitmap(StatusIcon.image(IconFrame(state: state, index: 0), badge: badge, mark: key.mark))
            if cache.count > 100 { cache.removeAll() }
            cache[key] = picture
        }
        image = picture
        onChange?()
    }

    private func watchSystem() {
        let ws = NSWorkspace.shared.notificationCenter
        let dist = DistributedNotificationCenter.default()
        func on(_ center: NotificationCenter, _ name: Notification.Name, _ body: @escaping @MainActor (MenuBarIcon) -> Void) {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    if let self {
                        body(self)
                        self.publish()
                    }
                }
            })
        }
        on(ws, NSWorkspace.screensDidSleepNotification) { $0.screensOff = true; $0.syncSleep() }
        on(ws, NSWorkspace.screensDidWakeNotification) { $0.screensOff = false; $0.syncSleep() }
        on(dist, Notification.Name("com.apple.screenIsLocked")) { $0.locked = true; $0.syncSleep() }
        on(dist, Notification.Name("com.apple.screenIsUnlocked")) { $0.locked = false; $0.syncSleep() }
        on(ws, NSWorkspace.accessibilityDisplayOptionsDidChangeNotification) {
            $0.animator.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        }
    }

    private func syncSleep() {
        animator.screensAsleep = screensOff || locked
    }
}
