import AppKit
import MossKit
import SwiftUI

/// `CCMoss --pet-sheets dir --bubbles`: Mo's real puff and message box,
/// placed beside the real layers exactly as on screen (the same placement
/// and margins), in both languages and both appearances. Also checks that
/// every bubble's shadow ends inside its window: a shadow cut off at the
/// window's edge shows as a rectangle.
enum BubbleSheets {
    @MainActor static func run(outDir: String) {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)
        func out(_ name: String) -> String { (outDir as NSString).appendingPathComponent(name) }
        var worst: CGFloat = 0
        for (lang, dark) in [(Lang.zh, false), (Lang.en, true), (Lang.zh, true), (Lang.en, false)] {
            L.lang = lang
            let tag = "\(lang == .zh ? "zh" : "en")-\(dark ? "dark" : "light")"
            let scenes = Scenes(dark: dark)
            PetSheets.snapshot(scenes.puffs, dark: dark, to: out("bubbles-puff-\(tag).png"))
            PetSheets.snapshot(scenes.boxes, dark: dark, to: out("bubbles-box-\(tag).png"))
            // The right-click bubble: busy and resting, above Mo and (at the top of a screen) below.
            let busy = PetMenu.content(needs: 2, broken: 0, resting: false, tucked: false) { _ in }
            let resting = PetMenu.content(needs: 0, broken: 0, resting: true, tucked: true) { _ in }
            PetSheets.snapshot(HStack(alignment: .top, spacing: 28) {
                PetMenuBubble(greeting: busy.greeting, groups: busy.groups, animate: false)
                PetMenuBubble(greeting: resting.greeting, groups: resting.groups,
                              tail: BubbleTail(x: 170, leansRight: false, onTop: true), animate: false)
            }
            .padding(30).background(Theme.surface(dark ? .dark : .light).opacity(1)),
            dark: dark, to: out("bubbles-menu-\(tag).png"))
            // Where you choose the mode: Settings › General › Desktop Pet.
            PetSheets.snapshot(VStack(alignment: .leading, spacing: 0) { PetSettings() }
                .padding(24).frame(width: 640).background(Theme.surface(dark ? .dark : .light).opacity(1)),
                dark: dark, to: out("bubbles-settings-\(tag).png"))
            // Each bubble's window on its own (transparent where clicks go through).
            for (i, bubble) in scenes.allBubbles.enumerated() {
                worst = max(worst, edgeAlpha(bubble, dark: dark, saveTo: out("bubbles-window-\(i + 1)-\(tag).png")))
            }
        }
        print(String(format: "shadow inside the window: %@ (largest alpha on a window edge %.3f)", worst < 0.01 ? "ok" : "CLIPPED", worst))
        exit(worst < 0.01 ? 0 : 1)
    }

    /// The most opaque pixel on the outer ring of the bubble's window.
    @MainActor static func edgeAlpha(_ view: AnyView, dark: Bool, saveTo path: String) -> CGFloat {
        let host = NSHostingView(rootView: view.environment(\.colorScheme, dark ? .dark : .light))
        host.frame = NSRect(origin: .zero, size: host.fittingSize)
        let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.backgroundColor = .clear
        window.isOpaque = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { return 1 }
        host.cacheDisplay(in: host.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
        var worst: CGFloat = 0
        let w = rep.pixelsWide, h = rep.pixelsHigh
        for x in 0..<w {
            for y in [0, h - 1] { worst = max(worst, rep.colorAt(x: x, y: y)?.alphaComponent ?? 0) }
        }
        for y in 0..<h {
            for x in [0, w - 1] { worst = max(worst, rep.colorAt(x: x, y: y)?.alphaComponent ?? 0) }
        }
        return worst
    }
}

/// The scenes: Mo on a desktop tile, its bubble where the app would put it.
@MainActor
private struct Scenes {
    let dark: Bool

    static let moSize: CGFloat = 84

    private func alert(_ id: Int, _ slot: String, _ kind: AlertKind, _ name: String, reason: String? = nil) -> SessionAlert {
        SessionAlert(id: id, slotId: slot, kind: kind, name: name, reason: reason, at: Date(timeIntervalSince1970: 0))
    }

    private func message(_ alerts: [SessionAlert]) -> PetMessage {
        var d = PetDirector(mode: .active)
        for (i, a) in alerts.enumerated() { d.take(a, at: Double(i) * 0.1) }
        return d.message ?? PetMessage(alerts[0])
    }

    private var needs: PetMessage { message([alert(1, "A", .needsYou, "api-review", reason: "permission")]) }
    private var question: PetMessage { message([alert(2, "B", .needsYou, "term-kit", reason: "question")]) }
    private var dropped: PetMessage { message([alert(3, "C", .interrupted, "fix-invoice")]) }
    private var many: PetMessage {
        message([alert(4, "D", .needsYou, "term-kit"), alert(5, "E", .needsYou, "billing")])
    }
    private var back: PetMessage {
        message([alert(6, "F", .restored, "api-review"), alert(7, "G", .restored, "billing"), alert(8, "H", .restored, "projects")])
    }

    private var update: PetMessage { PetMessage(UpdateTips.alert(version: "0.3.2", id: 9, at: Date(timeIntervalSince1970: 0))) }

    private func box(_ m: PetMessage) -> PetMessageBox {
        PetMessageBox(message: m, text: m.text(names: m.alerts.compactMap(\.name)), animate: false)
    }

    var allBubbles: [AnyView] {
        [AnyView(padded(PetPuff(text: Catchphrases.all(L.lang)[3], animate: false))),
         AnyView(padded(box(needs))), AnyView(padded(box(many)))]
    }

    private func padded<V: View>(_ v: V) -> some View { v.padding(PetBubbles.margin).fixedSize() }

    var puffs: some View {
        let phrases = Catchphrases.all(L.lang)
        return VStack(alignment: .leading, spacing: 14) {
            SheetTitle(title: L.t("Mo hints first", "小莫先示意"),
                       subtitle: L.t("Everyday mode: a face, a move and a translucent word or two. Hover for the message.",
                                     "日常模式：表情、动作和一两个字的半透明小气泡。悬停才展开消息框。"))
            HStack(spacing: 12) {
                scene(width: 300, height: 220, pose: .needsYou, moAtLeft: false) { tail in
                    PetPuff(text: phrases[0], tail: tail, animate: false)
                }
                scene(width: 300, height: 220, pose: .needsYou, moAtLeft: false) { tail in
                    PetPuff(text: phrases[2], tail: tail, animate: false)
                }
                scene(width: 300, height: 220, pose: .interrupted, moAtLeft: true) { tail in
                    PetPuff(text: phrases[3], tail: tail, animate: false)
                }
            }
        }
        .padding(24)
        .background(Theme.surface(dark ? .dark : .light).opacity(1))
    }

    var boxes: some View {
        VStack(alignment: .leading, spacing: 14) {
            SheetTitle(title: L.t("Mo's message box", "小莫的消息框"),
                       subtitle: L.t("On hover or click (active mode: straight away). Left of Mo, or right when there is no room.",
                                     "悬停或点击后出现（积极模式直接弹出）。默认在小莫左上方，左边放不下就翻到右边。"))
            HStack(spacing: 12) {
                scene(width: 460, height: 300, pose: .needsYou, moAtLeft: false) { tail in
                    PetMessageBox(message: needs, text: needs.text(names: ["api-review"]), tail: tail, animate: false)
                }
                scene(width: 460, height: 300, pose: .interrupted, moAtLeft: true) { tail in
                    PetMessageBox(message: dropped, text: dropped.text(names: ["fix-invoice"]), tail: tail, animate: false)
                }
            }
            HStack(spacing: 12) {
                scene(width: 460, height: 300, pose: .needsYou, moAtLeft: false) { tail in
                    PetMessageBox(message: many, text: many.text(names: ["term-kit", "billing"]), tail: tail, animate: false)
                }
                scene(width: 460, height: 300, pose: .happy, moAtLeft: false) { tail in
                    PetMessageBox(message: back, text: back.text(names: ["api-review", "billing", "projects"]), tail: tail, animate: false)
                }
            }
            HStack(spacing: 12) {
                scene(width: 460, height: 300, pose: .needsYou, moAtLeft: false) { tail in
                    PetMessageBox(message: question, text: question.text(names: ["term-kit"]), tail: tail, animate: false)
                }
                scene(width: 460, height: 300, pose: .idle, moAtLeft: false) { tail in
                    PetMessageBox(message: update, text: update.text(names: ["0.3.2"]), tail: tail, animate: false)
                }
            }
        }
        .padding(24)
        .background(Theme.surface(dark ? .dark : .light).opacity(1))
    }

    /// A desktop tile with Mo in a bottom corner and `bubble` placed by the
    /// app's own rules, the tile standing in for the screen.
    private func scene<B: View>(width: CGFloat, height: CGFloat, pose: PetPose, moAtLeft: Bool,
                                @ViewBuilder bubble: (BubbleTail) -> B) -> some View {
        let side = Self.moSize * 1.3
        let mo = CGPoint(x: moAtLeft ? 24 : width - side - 24, y: height - side - 8) // y-down
        let shape = PetView(body: (Self.moSize * 0.78).rounded()).hitShape.boundingBox
        let head = CGPoint(x: mo.x + shape.midX, y: mo.y + shape.minY - 8)
        let feet = CGPoint(x: mo.x + shape.midX, y: mo.y + shape.maxY + 4)
        let puff = B.self == PetPuff.self
        let measured = NSHostingView(rootView: bubble(BubbleTail()).padding(PetBubbles.margin).fixedSize()
            .environment(\.colorScheme, dark ? .dark : .light)).fittingSize
        // The tile is the screen; placement works y-up like AppKit.
        let place = BubblePlacement.place(size: measured, margin: PetBubbles.margin, inset: puff ? 12 : 34, corner: puff ? 4 : 14,
                                          above: CGPoint(x: head.x, y: height - head.y),
                                          below: CGPoint(x: feet.x, y: height - feet.y),
                                          visible: CGRect(x: 0, y: 0, width: width, height: height))
        let tail = BubbleTail(x: place.tailX, leansRight: place.leansRight, onTop: place.tailOnTop)
        return ZStack(alignment: .topLeading) {
            Desktop(dark: dark)
            LayerPet(pose: pose, size: Self.moSize, dark: dark)
                .frame(width: side, height: side)
                .offset(x: mo.x, y: mo.y)
            bubble(tail)
                .padding(PetBubbles.margin)
                .fixedSize()
                .offset(x: place.frame.minX.rounded(), y: (height - place.frame.maxY).rounded())
        }
        .frame(width: width, height: height, alignment: .topLeading)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}
