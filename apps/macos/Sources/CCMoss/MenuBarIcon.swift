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
    static let lineWidth: CGFloat = 2.1
    /// Working: how dim the mark is under the travelling spot, how long the
    /// spot's tail is (a fraction of the loop) and in how many fading pieces.
    static let dimmed: CGFloat = 0.4
    static let tail = 0.16
    static let pieces = 4
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
        guard let badge else { return NSSize(width: side, height: side) }
        let text = NSAttributedString(string: "\(badge)", attributes: [.font: font()])
        return NSSize(width: side + gap + ceil(text.size().width), height: side)
    }

    /// The icon for one frame, drawn on demand so it is sharp at any scale.
    /// `mark: false` leaves the mark's square empty (the animated layers sit
    /// there) and draws only the number.
    static func image(_ frame: IconFrame, badge: Int?, mark: Bool = true) -> NSImage {
        let size = size(badge: badge)
        let image = NSImage(size: size, flipped: true) { rect in
            guard let ctx = NSGraphicsContext.current?.cgContext else { return false }
            draw(frame, badge: badge, in: ctx, rect: rect, ink: .black, mark: mark)
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
    static func draw(_ frame: IconFrame, badge: Int?, in ctx: CGContext, rect: CGRect, ink: NSColor, mark drawMark: Bool = true) {
        let box = CGRect(x: rect.minX, y: rect.minY + (rect.height - side) / 2, width: side, height: side)
        let colour: NSColor = switch frame.state {
        case .needsYou: brand
        case .interrupted: amber
        default: ink
        }
        if drawMark { mark(frame, in: box, colour, ctx) }
        if let badge {
            let text = NSAttributedString(string: "\(badge)", attributes: [.font: font(), .foregroundColor: colour])
            let h = text.size().height
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
            text.draw(at: NSPoint(x: rect.minX + side + gap, y: rect.minY + (rect.height - h) / 2))
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
            flow(frame.progress, in: box, colour, ctx)
        case .needsYou:
            // A gentle breath: 1.0 → 1.08 → 1.0 over the loop.
            let s = 1 + 0.04 * (1 - cos(2 * .pi * frame.progress))
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

    /// Working: the mark dimmed, a brighter spot with a short fading tail
    /// travelling round it.
    private static func flow(_ head: Double, in box: CGRect, _ colour: NSColor, _ ctx: CGContext) {
        stroke(Continuum().path(in: box), colour, ctx, alpha: dimmed)
        for i in 0..<pieces {
            let from = head - tail + tail * Double(i) / Double(pieces)
            let to = head - tail + tail * Double(i + 1) / Double(pieces)
            stroke(Continuum.arc(in: box, from: from, to: to), colour, ctx, alpha: CGFloat(i + 1) / CGFloat(pieces))
        }
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

