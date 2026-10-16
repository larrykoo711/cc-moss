import AppKit
import MossKit
import QuartzCore

/// Mo, drawn with Core Animation: a tree of shape layers whose model values
/// come from a `PetLook`. Pose changes are implicit layer animations; loops
/// (breathing, the orbiting gap, zzz) are repeating layer animations that the
/// render server plays on its own, so this process does nothing per frame.
///
/// Coordinates are y-down (the root layer is flipped), matching the
/// prototype in `PetPrototype.swift`. `body` is Mo's width (48 / 64 / 88);
/// everything else is measured in `unit`, the prototype's `size`.
final class PetView: NSView {
    let body: CGFloat
    var unit: CGFloat { body / 0.78 }
    /// The square window Mo lives in: room for the antenna, hops and sparkles.
    static func side(body: CGFloat) -> CGFloat { (body / 0.78 * 1.3).rounded() }

    // Callbacks for the controller; the view itself decides nothing.
    var onClick: ((Int) -> Void)?
    var onDragEnded: (() -> Void)?
    var onRightClick: ((NSEvent) -> Void)?
    var onHover: ((Bool) -> Void)?
    var onPointer: ((CGPoint) -> Void)?

    private let stage = CALayer()
    private let floor = CAShapeLayer()
    private let hopper = CALayer()
    private let rig = CALayer()
    private let antenna = CALayer()
    private let stem = CAShapeLayer()
    private let lamp = CAShapeLayer()
    private let waves = CAShapeLayer()
    private let bodyShadow = CAShapeLayer()
    private let shell = CAGradientLayer()
    private let face = CALayer()
    private let loop = CAShapeLayer()
    private let arcs = CAShapeLayer()
    private let spark = CAShapeLayer()
    private let sparkleBig = CALayer()
    private let sparkleSmall = CALayer()
    private let zzz = CALayer()

    private(set) var look = PetLook.of(.idle)
    private var phase = 0.32
    private var tracking: NSTrackingArea?

    static let brand = NSColor(red: 1.0, green: 0.353, blue: 0.235, alpha: 1)
    static let amber = NSColor(red: 0.90, green: 0.68, blue: 0.30, alpha: 1)
    private static let graphiteTop = NSColor(red: 0.17, green: 0.18, blue: 0.21, alpha: 1)
    private static let graphiteBottom = NSColor(red: 0.08, green: 0.08, blue: 0.10, alpha: 1)

    init(body: CGFloat) {
        self.body = body
        let side = Self.side(body: body)
        super.init(frame: NSRect(x: 0, y: 0, width: side, height: side))
        // Layer-hosting: we own the tree, AppKit only shows it.
        let root = CALayer()
        root.isGeometryFlipped = true
        layer = root
        wantsLayer = true
        build()
        apply(look, animated: false)
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    override var isFlipped: Bool { true }

    // MARK: - Layout

    private var center: CGFloat { bounds.width / 2 }
    private var bodyRect: CGRect {
        CGRect(x: center - body / 2, y: center - body * 0.46, width: body, height: body * 0.92)
    }

    private func build() {
        guard let root = layer else { return }
        let b = bounds
        let u = unit
        let c = center
        for l in [stage, floor, hopper] { l.frame = b }
        root.addSublayer(stage)
        stage.addSublayer(floor)
        stage.addSublayer(hopper)

        // Floor shadow: only a shadow, no content.
        let ellipse = CGPath(ellipseIn: CGRect(x: c - body * 0.4, y: c + 0.44 * u - 0.035 * u,
                                               width: body * 0.8, height: 0.07 * u), transform: nil)
        floor.path = ellipse
        floor.fillColor = NSColor.black.withAlphaComponent(0.16).cgColor
        floor.shadowPath = ellipse
        floor.shadowColor = NSColor.black.cgColor
        floor.shadowOpacity = 0.2
        floor.shadowRadius = 3
        floor.shadowOffset = .zero

        // The rig scales and squashes from the feet.
        let feet = bodyRect.maxY
        rig.bounds = b
        rig.anchorPoint = CGPoint(x: 0.5, y: feet / b.height)
        rig.position = CGPoint(x: c, y: feet)
        hopper.addSublayer(rig)

        // Antenna: rotates about its foot.
        let aw = 0.12 * u, ah = 0.22 * u
        antenna.bounds = CGRect(x: 0, y: 0, width: aw, height: ah)
        antenna.anchorPoint = CGPoint(x: 0.5, y: 1)
        antenna.position = CGPoint(x: c, y: c - body * 0.5 - 0.06 * u + ah / 2)
        stem.path = CGPath(roundedRect: CGRect(x: aw / 2 - 0.015 * u, y: 0.05 * u, width: 0.03 * u, height: 0.16 * u),
                           cornerWidth: 0.015 * u, cornerHeight: 0.015 * u, transform: nil)
        stem.fillColor = NSColor.white.withAlphaComponent(0.28).cgColor
        lamp.path = CGPath(ellipseIn: CGRect(x: aw / 2 - 0.05 * u, y: 0, width: 0.10 * u, height: 0.10 * u), transform: nil)
        lamp.shadowRadius = 0.05 * u
        lamp.shadowOffset = .zero
        antenna.addSublayer(stem)
        antenna.addSublayer(lamp)
        rig.addSublayer(antenna)

        // Two small arcs by the antenna while it waves.
        // Like radio waves off the lamp, on its right.
        let wavePath = CGMutablePath()
        let lampCentre = CGPoint(x: c + 0.04 * u, y: c - body * 0.5 - 0.12 * u)
        for r in [0.11 * u, 0.18 * u] {
            let arc = CGMutablePath()
            arc.addArc(center: lampCentre, radius: r, startAngle: -.pi / 4, endAngle: .pi / 5, clockwise: false)
            wavePath.addPath(arc)
        }
        waves.path = wavePath
        waves.strokeColor = Self.brand.cgColor
        waves.fillColor = nil
        waves.lineWidth = max(1.2, 1.6 * u / 104)
        waves.lineCap = .round
        waves.opacity = 0
        rig.addSublayer(waves)

        // Body: a graphite pebble with its own soft shadow.
        let shellPath = CGPath(roundedRect: bodyRect, cornerWidth: body * 0.40, cornerHeight: body * 0.40, transform: nil)
        bodyShadow.path = shellPath
        bodyShadow.fillColor = Self.graphiteBottom.cgColor
        bodyShadow.shadowPath = shellPath
        bodyShadow.shadowColor = NSColor.black.cgColor
        bodyShadow.shadowOpacity = 0.28
        bodyShadow.shadowRadius = 0.05 * u
        bodyShadow.shadowOffset = CGSize(width: 0, height: 0.03 * u)
        rig.addSublayer(bodyShadow)
        shell.frame = bodyRect
        shell.colors = [Self.graphiteTop.cgColor, Self.graphiteBottom.cgColor]
        shell.startPoint = CGPoint(x: 0.5, y: 0)
        shell.endPoint = CGPoint(x: 0.5, y: 1)
        shell.cornerRadius = body * 0.40
        shell.cornerCurve = .continuous
        shell.masksToBounds = true
        shell.borderWidth = 1
        shell.borderColor = NSColor.white.withAlphaComponent(0.10).cgColor
        rig.addSublayer(shell)

        // The face: the loop for open eyes, two arcs for smiling or sleeping.
        let fs = body * 0.76
        face.frame = CGRect(x: c - fs / 2, y: c - fs / 2 - body * 0.03, width: fs, height: fs)
        for eye in [loop, arcs] {
            eye.frame = face.bounds
            eye.fillColor = nil
            eye.lineWidth = 0.065 * u
            eye.lineCap = .round
            eye.lineJoin = .round
            eye.shadowRadius = 0.04 * u
            eye.shadowOffset = .zero
            face.addSublayer(eye)
        }
        let sr = 0.025 * u
        spark.bounds = CGRect(x: 0, y: 0, width: 2 * sr, height: 2 * sr)
        spark.path = CGPath(ellipseIn: spark.bounds, transform: nil)
        spark.fillColor = NSColor.white.cgColor
        spark.shadowColor = Self.brand.cgColor
        spark.shadowOpacity = 1
        spark.shadowRadius = 3
        spark.shadowOffset = .zero
        face.addSublayer(spark)
        rig.addSublayer(face)

        // Extras around Mo.
        place(sparkleBig, symbol: "sparkle", size: 0.16 * u, at: CGPoint(x: c + 0.48 * u, y: c - 0.38 * u), color: Self.brand)
        place(sparkleSmall, symbol: "sparkle", size: 0.10 * u, at: CGPoint(x: c - 0.46 * u, y: c - 0.22 * u), color: Self.amber)
        place(zzz, symbol: "zzz", size: 0.15 * u, at: CGPoint(x: c + 0.46 * u, y: c - 0.40 * u), color: secondaryInk)
        for l in [sparkleBig, sparkleSmall, zzz] { hopper.addSublayer(l) }
    }

    /// Grey text colour for zzz, readable on light and dark desktops.
    private var secondaryInk: NSColor {
        effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            ? NSColor.white.withAlphaComponent(0.6) : NSColor.black.withAlphaComponent(0.5)
    }

    private func place(_ l: CALayer, symbol: String, size: CGFloat, at p: CGPoint, color: NSColor) {
        let config = NSImage.SymbolConfiguration(pointSize: size, weight: size > 0.12 * unit ? .bold : .semibold)
            .applying(NSImage.SymbolConfiguration(paletteColors: [color]))
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)?.withSymbolConfiguration(config)
        let s = image?.size ?? CGSize(width: size, height: size)
        l.bounds = CGRect(origin: .zero, size: s)
        l.position = p
        l.contents = image
        l.contentsGravity = .resizeAspect
        l.contentsScale = 2
        l.opacity = 0
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        let c = center, u = unit
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        place(zzz, symbol: "zzz", size: 0.15 * u, at: CGPoint(x: c + 0.46 * u, y: c - 0.40 * u), color: secondaryInk)
        zzz.opacity = look.accessory == .zzz ? 1 : 0
        CATransaction.commit()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        let scale = window?.backingScaleFactor ?? 2
        func visit(_ l: CALayer) {
            l.contentsScale = scale
            l.sublayers?.forEach(visit)
        }
        if let layer { visit(layer) }
    }

    // MARK: - Poses

    private func color(_ tint: PetTint) -> NSColor {
        switch tint {
        case .brand: Self.brand
        case .brandDim: Self.brand.withAlphaComponent(0.55)
        case .amber: Self.amber
        case .gray: NSColor.gray.withAlphaComponent(0.5)
        }
    }

    private var faceScale: CGFloat { face.bounds.width / CGFloat(PetGeometry.box) }

    private func path(_ points: [CGPoint]) -> CGPath {
        let p = CGMutablePath()
        let s = faceScale
        p.addLines(between: points.map { CGPoint(x: $0.x * s, y: $0.y * s) })
        return p
    }

    private func arcsPath(_ shape: PetShape) -> CGPath {
        let p = CGMutablePath()
        let s = faceScale
        for arc in shape.arcs { p.addLines(between: arc.map { CGPoint(x: $0.x * s, y: $0.y * s) }) }
        return p
    }

    /// Show a look. `animated` morphs the shapes (0.4 s); otherwise it is
    /// set at once, or, with `fade`, cross-faded in 0.2 s (Reduce Motion).
    /// `phase`, `blink` and `hop` pose a single moment, for still pictures.
    func apply(_ next: PetLook, animated: Bool, fade: Bool = false, phase: Double = 0.32, blink: Double = 1, hop: CGFloat = 0,
               wave: Double = 0) {
        look = next
        // Only the working gap travels; every other face shows its gap where it rests.
        let phase = next.spark ? phase : 0
        self.phase = phase
        CATransaction.begin()
        if animated {
            CATransaction.setAnimationDuration(0.4)
            CATransaction.setAnimationTimingFunction(CAMediaTimingFunction(name: .easeInEaseOut))
        } else {
            CATransaction.setDisableActions(true)
        }
        if fade {
            let t = CATransition()
            t.type = .fade
            t.duration = 0.2
            stage.add(t, forKey: "fade")
        }
        let shape = PetGeometry.shape(next.face.blinking(blink))
        let eye = color(next.eyes)
        loop.path = path(shape.loop)
        arcs.path = arcsPath(shape)
        let (start, end) = PetGeometry.stroke(next.face, phase: phase)
        loop.strokeStart = start
        loop.strokeEnd = end
        let isLoop = next.face.part == .loop
        loop.opacity = isLoop ? Float(next.eyeOpacity) : 0
        arcs.opacity = isLoop ? 0 : Float(next.eyeOpacity)
        for l in [loop, arcs] {
            l.strokeColor = eye.cgColor
            l.shadowColor = eye.cgColor
            l.shadowOpacity = next.glow ? 0.45 : 0
        }
        spark.opacity = next.spark ? 1 : 0
        spark.position = sparkPoint(next.face, phase: phase)

        let lampColor = color(next.lamp)
        lamp.fillColor = lampColor.cgColor
        lamp.shadowColor = lampColor.cgColor
        lamp.shadowOpacity = next.lamp == .gray ? 0 : 0.8
        antenna.transform = CATransform3DMakeRotation((next.antennaTilt + wave) * .pi / 180, 0, 0, 1)
        waves.opacity = wave != 0 && next.accessory == .waves ? 1 : 0
        rig.transform = CATransform3DMakeScale(next.squashX, next.squashY, 1)
        rig.opacity = Float(next.dim)
        hopper.transform = CATransform3DMakeTranslation(0, hop * unit, 0)
        floor.transform = CATransform3DMakeScale(hop == 0 ? 1 : 0.75, 1, 1)
        sparkleBig.opacity = next.accessory == .sparkles ? 1 : 0
        sparkleSmall.opacity = next.accessory == .sparkles ? 1 : 0
        zzz.opacity = next.accessory == .zzz ? 1 : 0
        CATransaction.commit()
        updateTracking()
    }

    private func sparkPoint(_ f: PetFace, phase: Double) -> CGPoint {
        let p = PetGeometry.point(atFraction: PetGeometry.head(f, phase: phase), of: f)
        return CGPoint(x: p.x * faceScale, y: p.y * faceScale)
    }

    /// Mo's opacity (DESIGN-pet §6.5): translucent at rest, solid when it wants you.
    func setPresence(_ opacity: Double, animated: Bool) {
        CATransaction.begin()
        if animated {
            CATransaction.setAnimationDuration(PetPresence.fade)
        } else {
            CATransaction.setDisableActions(true)
        }
        stage.opacity = Float(opacity)
        CATransaction.commit()
    }

    var presence: Double { Double(stage.opacity) }

    /// Eyes glance toward a point (view coordinates), at most 1.5 pt.
    func look(toward point: CGPoint?) {
        CATransaction.begin()
        CATransaction.setAnimationDuration(0.15)
        if let point {
            let dx = point.x - face.frame.midX, dy = point.y - face.frame.midY
            let d = max(hypot(dx, dy), 1)
            let reach = min(1.5, d / 12)
            face.transform = CATransform3DMakeTranslation(dx / d * reach, dy / d * reach, 0)
        } else {
            face.transform = CATransform3DIdentity
        }
        CATransaction.commit()
    }

    // MARK: - Loops (render server)

    private static func rate(_ fps: Float) -> CAFrameRateRange {
        CAFrameRateRange(minimum: min(10, fps), maximum: fps, preferred: fps)
    }

    /// Exactly these loops are attached afterwards; the rest are removed,
    /// not paused: nothing is left for the render server to play.
    func setLoops(_ loops: Set<PetLoop>) {
        if loops.contains(.breathe) {
            if rig.animation(forKey: "breathe") == nil {
                let a = CABasicAnimation(keyPath: "transform.scale")
                a.fromValue = 1
                a.toValue = 1.025
                a.duration = 2
                a.autoreverses = true
                a.repeatCount = .infinity
                a.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
                a.preferredFrameRateRange = Self.rate(PetLoop.breathe.maxFramesPerSecond)
                rig.add(a, forKey: "breathe")
            }
        } else {
            rig.removeAnimation(forKey: "breathe")
        }

        if loops.contains(.flow) {
            if loop.animation(forKey: "flow") == nil {
                let lap = 1 / Double(PetGeometry.laps)
                let (s, e) = PetGeometry.stroke(look.face, phase: phase)
                let group = CAAnimationGroup()
                let start = CABasicAnimation(keyPath: "strokeStart")
                start.fromValue = s
                start.toValue = s + lap
                let end = CABasicAnimation(keyPath: "strokeEnd")
                end.fromValue = e
                end.toValue = e + lap
                group.animations = [start, end]
                group.duration = 1.6
                group.repeatCount = .infinity
                group.timingFunction = CAMediaTimingFunction(name: .linear)
                group.preferredFrameRateRange = Self.rate(PetLoop.flow.maxFramesPerSecond)
                loop.add(group, forKey: "flow")

                let ride = CAKeyframeAnimation(keyPath: "position")
                ride.path = path(PetGeometry.lap(look.face, from: PetGeometry.head(look.face, phase: phase)))
                ride.calculationMode = .paced
                ride.duration = 1.6
                ride.repeatCount = .infinity
                ride.preferredFrameRateRange = Self.rate(PetLoop.flow.maxFramesPerSecond)
                spark.add(ride, forKey: "flow")
            }
        } else {
            loop.removeAnimation(forKey: "flow")
            spark.removeAnimation(forKey: "flow")
        }

        if loops.contains(.zzz) {
            if zzz.animation(forKey: "zzz") == nil {
                let rise = CAKeyframeAnimation(keyPath: "transform.translation.y")
                rise.values = [0.05 * unit, -0.05 * unit]
                let glow = CAKeyframeAnimation(keyPath: "opacity")
                glow.values = [0, 1, 1, 0]
                glow.keyTimes = [0, 0.3, 0.7, 1]
                let group = CAAnimationGroup()
                group.animations = [rise, glow]
                group.duration = 3
                group.repeatCount = .infinity
                group.preferredFrameRateRange = Self.rate(PetLoop.zzz.maxFramesPerSecond)
                zzz.add(group, forKey: "zzz")
            }
        } else {
            zzz.removeAnimation(forKey: "zzz")
        }
    }

    var runningLoops: [String] {
        [(rig, "breathe"), (loop, "flow"), (zzz, "zzz")].compactMap { $0.0.animation(forKey: $0.1) == nil ? nil : $0.1 }
    }

    // MARK: - One-off moves (60 fps, a second or so)

    private func oneShot(_ a: CAAnimation, duration: CFTimeInterval) {
        a.duration = duration
        a.preferredFrameRateRange = Self.rate(60)
    }

    /// Lobes squash to 0.1 and back in 0.18 s.
    func blink() {
        guard look.face.part == .loop else { return }
        let a = CAKeyframeAnimation(keyPath: "path")
        a.values = [1.0, 0.1, 1.0].map { path(PetGeometry.shape(look.face.blinking($0)).loop) }
        oneShot(a, duration: 0.18)
        loop.add(a, forKey: "blink")
    }

    /// Needs you: two hops; happy: one. The floor shadow shrinks in the air.
    func hop() {
        let heights: [CGFloat] = look.hops >= 2 ? [0, -0.06, -0.10, -0.03, 0, -0.07, 0] : [0, -0.07, 0]
        let duration = look.hops >= 2 ? 0.9 : 0.7
        let up = CAKeyframeAnimation(keyPath: "transform.translation.y")
        up.values = heights.map { $0 * unit }
        up.timingFunctions = (1..<heights.count).map { _ in CAMediaTimingFunction(name: .easeInEaseOut) }
        oneShot(up, duration: duration)
        hopper.add(up, forKey: "hop")
        let shrink = CAKeyframeAnimation(keyPath: "transform.scale.x")
        shrink.values = heights.map { 1 + $0 * 2.5 }
        oneShot(shrink, duration: duration)
        floor.add(shrink, forKey: "hop")
        if look.accessory == .sparkles {
            for l in [sparkleBig, sparkleSmall] {
                let pop = CASpringAnimation(keyPath: "transform.scale")
                pop.fromValue = 0.2
                pop.toValue = 1
                pop.damping = 9
                pop.stiffness = 220
                oneShot(pop, duration: pop.settlingDuration)
                l.add(pop, forKey: "pop")
            }
        }
    }

    /// The antenna waves (±22° for 3 s), or just once lightly (every minute
    /// while it waits).
    func wave(light: Bool = false) {
        let a = CAKeyframeAnimation(keyPath: "transform.rotation.z")
        let deg: [Double] = light ? [0, 12, -12, 0] : [0, 22, -22, 22, -22, 22, -22, 0]
        a.values = deg.map { $0 * .pi / 180 }
        a.isAdditive = true
        oneShot(a, duration: light ? 1.2 : 3)
        antenna.add(a, forKey: "wave")
        let marks = CAKeyframeAnimation(keyPath: "opacity")
        marks.values = [0, 1, 1, 0]
        marks.keyTimes = [0, 0.15, 0.85, 1]
        oneShot(marks, duration: light ? 1.2 : 3)
        waves.add(marks, forKey: "wave")
    }

    /// Reduce Motion stands in for hops and waves: the lamp blinks once.
    func flashLamp() {
        let a = CAKeyframeAnimation(keyPath: "opacity")
        a.values = [1, 0.15, 1]
        oneShot(a, duration: 0.6)
        lamp.add(a, forKey: "flash")
    }

    // MARK: - Mouse

    /// The part of the window that is Mo: the body and the antenna, a little
    /// generous. Everything else lets clicks through.
    var hitShape: CGPath {
        let p = CGMutablePath()
        p.addRoundedRect(in: bodyRect.insetBy(dx: -3, dy: -3), cornerWidth: body * 0.42, cornerHeight: body * 0.42)
        let u = unit
        p.addRect(CGRect(x: center - 0.08 * u, y: bodyRect.minY - 0.24 * u, width: 0.16 * u, height: 0.26 * u))
        return p
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let local = convert(point, from: superview)
        return hitShape.contains(local) ? self : nil
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private func updateTracking() {
        if let tracking { removeTrackingArea(tracking) }
        // Only over Mo itself: no events (and no work) while the pointer is elsewhere.
        let area = NSTrackingArea(rect: hitShape.boundingBox, options: [.mouseEnteredAndExited, .mouseMoved, .cursorUpdate, .activeAlways],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        updateTracking()
    }

    // The pointing hand over Mo (like every clickable thing in the app); the
    // arrow again over the transparent surroundings. Set directly: cursor
    // rects only work in the key window, and Mo's never is.
    override func mouseEntered(with event: NSEvent) {
        NSCursor.pointingHand.set()
        onHover?(true)
    }

    override func mouseExited(with event: NSEvent) {
        NSCursor.arrow.set()
        onHover?(false)
    }

    override func mouseMoved(with event: NSEvent) {
        NSCursor.pointingHand.set()
        onPointer?(convert(event.locationInWindow, from: nil))
    }

    override func cursorUpdate(with event: NSEvent) { NSCursor.pointingHand.set() }

    private var press: (mouse: NSPoint, origin: NSPoint)?
    private var dragging = false

    override func mouseDown(with event: NSEvent) {
        guard let window else { return }
        press = (NSEvent.mouseLocation, window.frame.origin)
        dragging = false
    }

    override func mouseDragged(with event: NSEvent) {
        guard let press, let window else { return }
        let now = NSEvent.mouseLocation
        let dx = now.x - press.mouse.x, dy = now.y - press.mouse.y
        if !dragging, hypot(dx, dy) < 3 { return }
        dragging = true
        window.setFrameOrigin(NSPoint(x: press.origin.x + dx, y: press.origin.y + dy))
    }

    override func mouseUp(with event: NSEvent) {
        defer { press = nil }
        if dragging {
            dragging = false
            onDragEnded?()
        } else {
            onClick?(event.clickCount)
        }
    }

    override func rightMouseDown(with event: NSEvent) { onRightClick?(event) }

    // MARK: - Pictures

    /// The layer tree drawn into a bitmap, for `--pet-sheets --layers`.
    /// Model values only: a still moment, not a running animation.
    func snapshot(scale: CGFloat = 2) -> NSImage {
        let size = bounds.size
        let image = NSImage(size: size)
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ), let gc = NSGraphicsContext(bitmapImageRep: rep), let layer else { return image }
        rep.size = size
        let ctx = gc.cgContext
        ctx.scaleBy(x: scale, y: scale)
        // CALayer.render draws y-up; the root is flipped, so flip back.
        ctx.translateBy(x: 0, y: size.height)
        ctx.scaleBy(x: 1, y: -1)
        layer.render(in: ctx)
        image.addRepresentation(rep)
        return image
    }
}
