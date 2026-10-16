import AppKit
import MossKit
import SwiftUI

/// The menu bar icon's pictures: the Continuum mark in each `IconState`,
/// optionally followed by a compact number (iStat Menus style).
///
/// Quiet states are template images, so the menu bar tints them for light
/// and dark; the two that want your attention carry their own colour.
enum StatusIcon {
    static let side: CGFloat = 18
    /// Room either side of the mark for its stroke and the needs-you beat.
    static let margin: CGFloat = 0.75
    /// As heavy as the menu bar icons around it (18 pt, about 2.7 pt strokes).
    static let lineWidth: CGFloat = 2.7
    /// Space between the mark and the number.
    static let gap: CGFloat = 3

    /// The two coloured states: brand red-orange asks for you, amber says
    /// something broke (deeper than the panel's amber so it holds up on a
    /// light menu bar).
    static let brand = NSColor(red: 1.0, green: 0.353, blue: 0.235, alpha: 1)
    static let amber = NSColor(red: 0.93, green: 0.62, blue: 0.10, alpha: 1)

    /// Template unless the state has its own colour.
    static func isTemplate(_ state: IconState) -> Bool {
        state != .needsYou && state != .interrupted
    }

    static func font() -> NSFont {
        .monospacedDigitSystemFont(ofSize: 12, weight: .semibold)
    }

    static func size(badge: Int?) -> NSSize {
        guard let badge else { return NSSize(width: side + 2 * margin, height: side) }
        let text = NSAttributedString(string: "\(badge)", attributes: [.font: font()])
        return NSSize(width: side + 2 * margin + gap + ceil(text.size().width), height: side)
    }

    /// The icon for one frame, drawn on demand so it is sharp at any scale.
    static func image(_ frame: IconFrame, badge: Int?) -> NSImage {
        let size = size(badge: badge)
        let image = NSImage(size: size, flipped: true) { rect in
            guard let ctx = NSGraphicsContext.current?.cgContext else { return false }
            draw(frame, badge: badge, in: ctx, rect: rect, ink: .black)
            return true
        }
        image.isTemplate = isTemplate(frame.state)
        image.accessibilityDescription = frame.state.label
        return image
    }

    /// The same picture as a 2x bitmap, so showing it costs no drawing.
    static func bitmap(_ source: NSImage) -> NSImage {
        let size = source.size
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width * 2), pixelsHigh: Int(size.height * 2),
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ) else { return source }
        rep.size = size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        source.draw(in: NSRect(origin: .zero, size: size))
        NSGraphicsContext.restoreGraphicsState()
        let out = NSImage(size: size)
        out.addRepresentation(rep)
        out.isTemplate = source.isTemplate
        out.accessibilityDescription = source.accessibilityDescription
        return out
    }

    /// Draws into a y-down context. `ink` is what template parts are drawn
    /// with: black for the real (tinted) icon, the bar's text colour in previews.
    static func draw(_ frame: IconFrame, badge: Int?, in ctx: CGContext, rect: CGRect, ink: NSColor) {
        let box = CGRect(x: rect.minX + margin, y: rect.minY + (rect.height - side) / 2, width: side, height: side)
        let colour: NSColor = switch frame.state {
        case .needsYou: brand
        case .interrupted: amber
        default: ink
        }
        mark(frame, in: box, colour, ctx)
        if let badge {
            let text = NSAttributedString(string: "\(badge)", attributes: [.font: font(), .foregroundColor: colour])
            let h = text.size().height
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
            text.draw(at: NSPoint(x: rect.minX + side + 2 * margin + gap, y: rect.minY + (rect.height - h) / 2))
            NSGraphicsContext.restoreGraphicsState()
        }
    }

    private static func mark(_ frame: IconFrame, in box: CGRect, _ colour: NSColor, _ ctx: CGContext) {
        ctx.saveGState()
        // Its own layer, so the waiting dot's clear ring cuts only the mark.
        ctx.beginTransparencyLayer(auxiliaryInfo: nil)
        defer {
            ctx.endTransparencyLayer()
            ctx.restoreGState()
        }
        ctx.setLineCap(.round)
        ctx.setLineJoin(.round)
        ctx.setLineWidth(lineWidth)
        switch frame.state {
        case .idle, .interrupted:
            stroke(Continuum().path(in: box), colour, ctx)
        case .waiting:
            stroke(Continuum().path(in: box), colour, ctx)
            dot(in: box, colour, ctx)
        case .working:
            flow(frame.index, in: box, colour, ctx)
        case .needsYou:
            // A calm heartbeat: rest, then a beat at 1.07.
            let s: CGFloat = frame.index % 2 == 1 ? 1.07 : 1
            ctx.translateBy(x: box.midX, y: box.midY)
            ctx.scaleBy(x: s, y: s)
            ctx.translateBy(x: -box.midX, y: -box.midY)
            stroke(Continuum().path(in: box), colour, ctx)
        }
    }

    private static func stroke(_ path: Path, _ colour: NSColor, _ ctx: CGContext, alpha: CGFloat = 1) {
        ctx.addPath(path.cgPath)
        ctx.setStrokeColor(colour.withAlphaComponent(alpha).cgColor)
        ctx.strokePath()
    }

    /// Working: the mark slightly dimmed and a full-strength dash that
    /// ticks round it, one of 12 evenly spaced positions per step: a slow
    /// progress dot, not a choppy animation.
    private static func flow(_ index: Int, in box: CGRect, _ colour: NSColor, _ ctx: CGContext) {
        let n = Double(IconMotion.frameCount(.working))
        let centre = Double(index) / n
        stroke(Continuum().path(in: box), colour, ctx, alpha: 0.62)
        // A little longer and heavier than the ring, so it reads at 1x too.
        let half = 0.045
        ctx.setLineWidth(lineWidth + 0.7)
        stroke(Continuum.arc(in: box, from: centre - half, to: centre + half), colour, ctx)
    }

    /// Waiting: a small dot at the top right, with a clear ring round it so
    /// it never touches the mark.
    private static func dot(in box: CGRect, _ colour: NSColor, _ ctx: CGContext) {
        // In the corner above the right lobe: about 3 pt clear of the stroke.
        let r: CGFloat = 1.7
        let c = CGPoint(x: box.maxX - 1.8, y: box.minY + 1.8)
        ctx.saveGState()
        ctx.setBlendMode(.clear)
        ctx.fillEllipse(in: CGRect(x: c.x - r - 0.8, y: c.y - r - 0.8, width: 2 * (r + 0.8), height: 2 * (r + 0.8)))
        ctx.restoreGState()
        ctx.setFillColor(colour.cgColor)
        ctx.fillEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
    }
}

/// Runs the menu bar's timer: a plain run-loop timer, also ticking while a
/// menu is open, with some tolerance so the system can batch wake-ups.
final class RunLoopTicks: TickScheduler {
    private var timer: Timer?

    func start(interval: TimeInterval, _ tick: @escaping () -> Void) {
        timer?.invalidate()
        let t = Timer(timeInterval: interval, repeats: true) { _ in tick() }
        t.tolerance = interval * 0.25
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }
}

/// The status item's picture: what the sessions say, the system's motion
/// and sleep state, and the frame that results. Deliberately not observable:
/// each frame goes straight to `onImage` (the status button), with no
/// SwiftUI update on the per-frame path.
@MainActor
final class MenuBarIcon {
    private(set) var image: NSImage
    private(set) var state: IconState = .idle
    /// Called with every new picture.
    var onImage: ((NSImage) -> Void)?

    private let animator: IconAnimator
    private var badge: Int?
    private var observers: [NSObjectProtocol] = []
    private var screensOff = false
    private var locked = false

    init(scheduler: TickScheduler = RunLoopTicks()) {
        animator = IconAnimator(clock: { ProcessInfo.processInfo.systemUptime }, scheduler: scheduler)
        image = NSImage()
        animator.onFrame = { [weak self] frame in
            MainActor.assumeIsolated { self?.redraw(frame) }
        }
        redraw(animator.frame)
        animator.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        watchSystem()
    }

    /// New sessions from a refresh; `showCount` is the "Icon and number" setting.
    func update(_ sessions: [Session], showCount: Bool) {
        let nextBadge = showCount ? IconBadge.count(sessions) : nil
        let badgeChanged = nextBadge != badge
        badge = nextBadge
        state = IconState(sessions)
        let before = animator.frame
        animator.state = state
        // The animator only reports frame changes; a new number alone needs a redraw too.
        if badgeChanged, animator.frame == before { redraw(animator.frame) }
    }

    /// Every picture is drawn once and reused: an animation loops through
    /// the same few frames, and drawing each anew kept the CPU busy.
    private var cache: [CacheKey: NSImage] = [:]

    private struct CacheKey: Hashable {
        let frame: IconFrame
        let badge: Int?
    }

    private func redraw(_ frame: IconFrame) {
        let key = CacheKey(frame: frame, badge: badge)
        let picture: NSImage
        if let hit = cache[key] {
            picture = hit
        } else {
            // Bake to a bitmap now; a drawing-handler image would redraw itself
            // every time the menu bar displays it.
            picture = StatusIcon.bitmap(StatusIcon.image(frame, badge: badge))
            if cache.count > 200 { cache.removeAll() }
            cache[key] = picture
        }
        guard picture !== image else { return }
        image = picture
        onImage?(picture)
    }

    private func watchSystem() {
        let ws = NSWorkspace.shared.notificationCenter
        let dist = DistributedNotificationCenter.default()
        func on(_ center: NotificationCenter, _ name: Notification.Name, _ body: @escaping @MainActor (MenuBarIcon) -> Void) {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { if let self { body(self) } }
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
