import AppKit
import MossKit
import SwiftUI

// Design prototype for the desktop pet (T3.1). Static poses only: it exists to
// render the sheets in `.sdd/002-menubar/diagrams/pet/` and is not wired into
// the app. The real pet (T3.2–T3.4) will reuse the geometry, not this file.
//
//   CCMoss --pet-sheets <out-dir>
//
// Everything is drawn from the Continuum (x = 32 + 27 cos t,
// y = 32 + 12.9 sin 2t in a 64-unit box) plus a few SF Symbols.

// MARK: - Geometry

/// The Continuum used as a face or a body: each lobe can be squashed (blink,
/// wide eyes), the outer ends can droop (worry), and either half can be drawn
/// alone (happy arcs, sleeping arcs).
struct LoopFace: Shape {
    enum Part { case full, upper, lower }

    var part: Part = .full
    var amp = 12.9
    var left = 1.0
    var right = 1.0
    /// Outer ends move down by this much (64-unit box): a worried face.
    var droop = 0.0
    /// The gap, as fractions of the path; `gapStart == gapEnd` closes it.
    var gapStart = 0.695
    var gapEnd = 0.805
    var phase = 0.0

    private func point(_ t: Double, in rect: CGRect) -> CGPoint {
        let c = cos(t)
        let scale = c < 0 ? left : right
        let x = 32 + 27 * c
        let y = 32 + amp * sin(2 * t) * scale + droop * pow(abs(c), 3)
        return CGPoint(x: rect.minX + x / 64 * rect.width, y: rect.minY + y / 64 * rect.height)
    }

    private func polyline(_ from: Double, _ to: Double, in rect: CGRect) -> Path {
        Path { p in
            let steps = 80
            for i in 0...steps {
                let t = from + (to - from) * Double(i) / Double(steps)
                i == 0 ? p.move(to: point(t, in: rect)) : p.addLine(to: point(t, in: rect))
            }
        }
    }

    func path(in rect: CGRect) -> Path {
        switch part {
        case .upper:
            // sin 2t < 0: the top half of each lobe, an arch over each eye.
            var p = polyline(.pi / 2 + 0.25, .pi, in: rect)
            p.addPath(polyline(1.5 * .pi + 0.25, 2 * .pi, in: rect))
            return p
        case .lower:
            var p = polyline(.pi + 0.0, 1.5 * .pi - 0.25, in: rect)
            p.addPath(polyline(0, .pi / 2 - 0.25, in: rect))
            return p
        case .full:
            let full = polyline(0, 2 * .pi, in: rect)
            if gapStart == gapEnd { return full }
            let from = (gapEnd + phase).truncatingRemainder(dividingBy: 1)
            let to = (gapStart + phase).truncatingRemainder(dividingBy: 1)
            if from < to { return full.trimmedPath(from: from, to: to) }
            var out = full.trimmedPath(from: from, to: 1)
            out.addPath(full.trimmedPath(from: 0, to: to))
            return out
        }
    }

    /// Where a fraction of the path sits, for sparks that ride the loop.
    func spot(_ fraction: Double, in rect: CGRect) -> CGPoint {
        point(fraction * 2 * .pi, in: rect)
    }
}

// MARK: - Poses

enum PetPose: String, CaseIterable {
    case idle, working, needsYou, interrupted, happy, sleeping

    var title: String {
        switch self {
        case .idle: L.t("Idle", "待机")
        case .working: L.t("Working", "在忙")
        case .needsYou: L.t("Needs you", "需要你")
        case .interrupted: L.t("Interrupted", "断了")
        case .happy: L.t("Back / done", "找回 / 完成")
        case .sleeping: L.t("Do not disturb", "勿扰")
        }
    }

    var caption: String {
        switch self {
        case .idle: L.t("breathes 4 s, blinks every 6–12 s", "呼吸 4 秒一次，6–12 秒随机眨眼")
        case .working: L.t("the gap orbits the eyes, 1.6 s/lap", "缺口绕着眼睛转，1.6 秒一圈")
        case .needsYou: L.t("antenna waves, two hops, then waits", "天线招手，跳两下，然后等着")
        case .interrupted: L.t("loop breaks, eyes droop, amber", "环断开，眼角下垂，琥珀色")
        case .happy: L.t("smiling arcs, one hop, sparkles", "眯眼笑，跳一下，闪两颗星")
        case .sleeping: L.t("eyes shut, dimmed, slow zzz", "闭眼、变暗，慢慢冒 zzz")
        }
    }
}

enum PetDirection: String, CaseIterable {
    case ribbon, pebble, orbit

    var title: String {
        switch self {
        case .ribbon: L.t("A · Ribbon", "A · 环")
        case .pebble: L.t("B · Mo (recommended)", "B · 小莫（推荐）")
        case .orbit: L.t("C · Orbit", "C · 轨道")
        }
    }

    var blurb: String {
        switch self {
        case .ribbon: L.t("The mark itself, alive. Purest brand, least expressive.", "标志本身活过来。最纯粹，表情最少。")
        case .pebble: L.t("A pebble with the loop as its eyes. Reads at 64 pt.", "一颗小圆石，无限环是它的眼睛。64 pt 也看得清。")
        case .orbit: L.t("A small planet, the loop as its orbit. Most cinematic.", "一颗小星球，无限环是它的轨道。最有电影感。")
        }
    }
}

private let graphiteTop = Color(red: 0.17, green: 0.18, blue: 0.21)
private let graphiteBottom = Color(red: 0.08, green: 0.08, blue: 0.10)

// MARK: - Direction B: Mo, the pebble

struct PebblePet: View {
    let pose: PetPose
    var size: CGFloat = 96

    private var eyeColor: Color { pose == .interrupted ? Theme.amber : Theme.brand }

    private var face: LoopFace {
        switch pose {
        case .idle: LoopFace()
        case .working: LoopFace(phase: 0.32)
        case .needsYou: LoopFace(amp: 15.5, left: 1.1, right: 1.1, gapStart: 0.72, gapEnd: 0.78)
        case .interrupted: LoopFace(amp: 11, droop: 5, gapStart: 0.60, gapEnd: 0.90)
        case .happy: LoopFace(part: .upper, amp: 16)
        case .sleeping: LoopFace(part: .lower, amp: 6)
        }
    }

    private var hop: CGFloat {
        switch pose {
        case .needsYou: -size * 0.10
        case .happy: -size * 0.07
        default: 0
        }
    }

    private var antennaTilt: Double {
        switch pose {
        case .needsYou: 28
        case .interrupted: -32
        case .sleeping: -14
        default: 0
        }
    }

    private var antennaColor: Color {
        switch pose {
        case .idle: Theme.brand.opacity(0.55)
        case .working, .happy: Theme.brand
        case .needsYou: Theme.brand
        case .interrupted: Theme.amber
        case .sleeping: Color.gray.opacity(0.5)
        }
    }

    var body: some View {
        let body = size * 0.78
        ZStack {
            // Floor shadow: shrinks while the body is in the air.
            Ellipse()
                .fill(Color.black.opacity(0.22))
                .frame(width: body * (hop == 0 ? 0.8 : 0.6), height: size * 0.07)
                .blur(radius: 3)
                .offset(y: size * 0.44)
            ZStack {
                // Antenna: a stem and a light. The light is the pet's status lamp.
                ZStack(alignment: .top) {
                    Capsule().fill(Color.white.opacity(0.28))
                        .frame(width: size * 0.03, height: size * 0.16)
                        .offset(y: size * 0.05)
                    Circle().fill(antennaColor)
                        .frame(width: size * 0.10, height: size * 0.10)
                        .shadow(color: antennaColor.opacity(pose == .sleeping ? 0 : 0.8), radius: size * 0.05)
                }
                .frame(height: size * 0.22, alignment: .top)
                .rotationEffect(.degrees(antennaTilt), anchor: .bottom)
                .offset(y: -body * 0.5 - size * 0.06)
                if pose == .needsYou {
                    WaveMarks(color: Theme.brand)
                        .frame(width: size * 0.34, height: size * 0.14)
                        .offset(x: size * 0.16, y: -body * 0.5 - size * 0.16)
                }
                RoundedRectangle(cornerRadius: body * 0.40, style: .continuous)
                    .fill(LinearGradient(colors: [graphiteTop, graphiteBottom], startPoint: .top, endPoint: .bottom))
                    .overlay(
                        RoundedRectangle(cornerRadius: body * 0.40, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.10), lineWidth: 1)
                    )
                    .frame(width: body, height: body * 0.92)
                    .shadow(color: .black.opacity(0.28), radius: size * 0.05, y: size * 0.03)
                face
                    .stroke(eyeColor, style: StrokeStyle(lineWidth: size * 0.065, lineCap: .round, lineJoin: .round))
                    .frame(width: body * 0.76, height: body * 0.76)
                    .shadow(color: eyeColor.opacity(pose == .sleeping ? 0 : 0.45), radius: size * 0.04)
                    .opacity(pose == .sleeping ? 0.55 : 1)
                    .offset(y: -body * 0.03)
                if pose == .working {
                    // The spark at the head of the moving gap.
                    let rect = CGRect(x: 0, y: 0, width: body * 0.76, height: body * 0.76)
                    let p = face.spot(0.805 + 0.32 - 1, in: rect)
                    Circle().fill(Color.white)
                        .frame(width: size * 0.05, height: size * 0.05)
                        .shadow(color: Theme.brand, radius: 3)
                        .position(p)
                        .frame(width: body * 0.76, height: body * 0.76)
                        .offset(y: -body * 0.03)
                }
            }
            .scaleEffect(x: pose == .interrupted ? 1.03 : 1, y: pose == .interrupted ? 0.95 : 1, anchor: .bottom)
            .opacity(pose == .sleeping ? 0.82 : 1)
            .offset(y: hop)
            accessory
        }
        .frame(width: size * 1.3, height: size * 1.3)
    }

    @ViewBuilder private var accessory: some View {
        switch pose {
        case .happy:
            Image(systemName: "sparkle").font(.system(size: size * 0.16, weight: .bold))
                .foregroundStyle(Theme.brand).offset(x: size * 0.48, y: -size * 0.38)
            Image(systemName: "sparkle").font(.system(size: size * 0.10, weight: .bold))
                .foregroundStyle(Theme.amber).offset(x: -size * 0.46, y: -size * 0.22)
        case .sleeping:
            Image(systemName: "zzz").font(.system(size: size * 0.15, weight: .semibold))
                .foregroundStyle(.secondary).offset(x: size * 0.46, y: -size * 0.40)
        default:
            EmptyView()
        }
    }
}

/// Two small arcs beside the antenna: it is waving.
private struct WaveMarks: View {
    let color: Color
    var body: some View {
        HStack(spacing: 2) {
            ForEach(0..<2) { i in
                Circle().trim(from: 0.05, to: 0.30)
                    .stroke(color.opacity(0.9 - Double(i) * 0.35), style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
                    .rotationEffect(.degrees(-50))
                    .frame(width: 10 + CGFloat(i) * 6, height: 10 + CGFloat(i) * 6)
            }
        }
    }
}

// MARK: - Direction A: Ribbon

struct RibbonPet: View {
    let pose: PetPose
    var size: CGFloat = 96

    var body: some View {
        let w = size * 1.05
        let color: Color = pose == .interrupted ? Theme.amber : (pose == .sleeping ? .gray : Theme.brand)
        let shape: LoopFace = switch pose {
        case .idle: LoopFace()
        case .working: LoopFace(phase: 0.32)
        case .needsYou: LoopFace(amp: 16, gapStart: 0.72, gapEnd: 0.78)
        case .interrupted: LoopFace(amp: 10, droop: 6, gapStart: 0.55, gapEnd: 0.95)
        case .happy: LoopFace(gapStart: 0.5, gapEnd: 0.5)
        case .sleeping: LoopFace(amp: 5)
        }
        ZStack {
            shape
                .stroke(
                    LinearGradient(colors: [color, color.opacity(0.75)], startPoint: .leading, endPoint: .trailing),
                    style: StrokeStyle(lineWidth: size * 0.13, lineCap: .round, lineJoin: .round)
                )
                .frame(width: w, height: w)
                .shadow(color: color.opacity(pose == .sleeping ? 0 : 0.45), radius: size * 0.06)
                .opacity(pose == .sleeping ? 0.5 : 1)
                .offset(y: pose == .needsYou ? -size * 0.08 : 0)
                .rotationEffect(.degrees(pose == .needsYou ? -8 : 0))
            if pose == .working {
                let rect = CGRect(x: 0, y: 0, width: w, height: w)
                Circle().fill(Color.white)
                    .frame(width: size * 0.09, height: size * 0.09)
                    .shadow(color: Theme.brand, radius: 4)
                    .position(shape.spot(0.805 + 0.32 - 1, in: rect))
                    .frame(width: w, height: w)
            }
            if pose == .happy {
                Image(systemName: "sparkle").font(.system(size: size * 0.16, weight: .bold))
                    .foregroundStyle(Theme.brand).offset(x: size * 0.44, y: -size * 0.32)
            }
            if pose == .sleeping {
                Image(systemName: "zzz").font(.system(size: size * 0.15, weight: .semibold))
                    .foregroundStyle(.secondary).offset(x: size * 0.42, y: -size * 0.30)
            }
        }
        .frame(width: size * 1.3, height: size * 1.3)
    }
}

// MARK: - Direction C: Orbit

struct OrbitPet: View {
    let pose: PetPose
    var size: CGFloat = 96

    var body: some View {
        let ball = size * 0.50
        let w = size * 1.15
        let orbit: LoopFace = switch pose {
        case .interrupted: LoopFace(gapStart: 0.55, gapEnd: 0.95)
        case .sleeping: LoopFace(amp: 7, gapStart: 0.5, gapEnd: 0.5)
        default: LoopFace(gapStart: 0.5, gapEnd: 0.5)
        }
        let satellite: Double = switch pose {
        case .working: 0.12
        case .needsYou: 0.0
        default: 0.30
        }
        let satColor: Color = pose == .interrupted ? Theme.amber : Theme.brand
        ZStack {
            orbit
                .stroke(Color.white.opacity(0.32), style: StrokeStyle(lineWidth: 1.6, lineCap: .round, dash: pose == .working ? [3, 4] : []))
                .frame(width: w, height: w)
            Circle()
                .fill(RadialGradient(
                    colors: [Color(red: 0.42, green: 0.58, blue: 0.82), Color(red: 0.11, green: 0.17, blue: 0.30)],
                    center: UnitPoint(x: 0.35, y: 0.3), startRadius: 1, endRadius: ball * 0.75
                ))
                .overlay(Circle().strokeBorder(Color.white.opacity(0.12), lineWidth: 1))
                .frame(width: ball, height: ball)
                .shadow(color: .black.opacity(0.3), radius: 4, y: 2)
                .opacity(pose == .sleeping ? 0.75 : 1)
            OrbitEyes(pose: pose).frame(width: ball * 0.56, height: ball * 0.3).offset(y: -ball * 0.04)
            if pose != .sleeping {
                let rect = CGRect(x: 0, y: 0, width: w, height: w)
                Circle().fill(satColor)
                    .frame(width: size * (pose == .needsYou ? 0.16 : 0.10), height: size * (pose == .needsYou ? 0.16 : 0.10))
                    .shadow(color: satColor, radius: pose == .needsYou ? 8 : 4)
                    .position(orbit.spot(satellite, in: rect))
                    .frame(width: w, height: w)
            }
            if pose == .happy {
                Image(systemName: "sparkle").font(.system(size: size * 0.14, weight: .bold))
                    .foregroundStyle(Theme.brand).offset(x: -size * 0.40, y: -size * 0.36)
            }
            if pose == .sleeping {
                Image(systemName: "zzz").font(.system(size: size * 0.14, weight: .semibold))
                    .foregroundStyle(.secondary).offset(x: size * 0.36, y: -size * 0.36)
            }
        }
        .frame(width: size * 1.3, height: size * 1.3)
    }
}

private struct OrbitEyes: View {
    let pose: PetPose
    var body: some View {
        HStack {
            eye(mirror: false)
            Spacer()
            eye(mirror: true)
        }
    }

    @ViewBuilder private func eye(mirror: Bool) -> some View {
        switch pose {
        case .happy:
            Circle().trim(from: 0.55, to: 0.95).stroke(Color.white, style: StrokeStyle(lineWidth: 2.2, lineCap: .round))
                .frame(width: 10, height: 10)
        case .sleeping:
            Circle().trim(from: 0.05, to: 0.45).stroke(Color.white.opacity(0.8), style: StrokeStyle(lineWidth: 2, lineCap: .round))
                .frame(width: 9, height: 9)
        case .interrupted:
            Capsule().fill(Color.white).frame(width: 4, height: 9)
                .rotationEffect(.degrees(mirror ? -20 : 20))
        case .needsYou:
            Capsule().fill(Color.white).frame(width: 5.5, height: 13)
        default:
            Capsule().fill(Color.white).frame(width: 4.5, height: 10)
        }
    }
}

// MARK: - Speech bubble

/// One message from the pet. The footer is where 004 (approve in the bubble)
/// and 005 (talk back) will plug in; here they are shown as reserved slots.
struct PetBubble: View {
    enum Kind { case needsYou, finished, interrupted, restored, merged }

    let kind: Kind
    let title: String
    let message: String
    var actions: [String] = []
    var speaking = false
    var reserved: String?
    @Environment(\.colorScheme) private var scheme

    private var tint: Color {
        switch kind {
        case .needsYou: Theme.brand
        case .interrupted: Theme.amber
        case .finished, .merged: Color.secondary
        case .restored: Color.green
        }
    }

    private var symbol: String {
        switch kind {
        case .needsYou: "hand.raised"
        case .finished: Icons.state(.waiting)
        case .interrupted: Icons.state(.interrupted)
        case .restored: "checkmark.circle"
        case .merged: "square.stack"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6).fill(tint.opacity(scheme == .dark ? 0.22 : 0.14))
                    Image(systemName: symbol).font(.system(size: 11, weight: .semibold)).foregroundStyle(tint)
                }
                .frame(width: 22, height: 22)
                Text(title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Spacer(minLength: 6)
                if speaking {
                    Image(systemName: "speaker.wave.2.fill")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                }
                Image(systemName: "xmark").font(.system(size: 9, weight: .bold)).foregroundStyle(.tertiary)
            }
            Text(message).font(.system(size: 13)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            if !actions.isEmpty {
                HStack(spacing: 6) {
                    ForEach(Array(actions.enumerated()), id: \.offset) { i, a in
                        Text(a)
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(i == 0 ? Color.white : Color.primary)
                            .padding(.horizontal, 10).frame(height: 24)
                            .background(Capsule().fill(i == 0 ? AnyShapeStyle(tint) : AnyShapeStyle(Theme.selection(scheme))))
                    }
                }
                .padding(.top, 2)
            }
            if let reserved {
                Text(reserved)
                    .font(.system(size: 11)).foregroundStyle(.tertiary)
                    .frame(maxWidth: .infinity).frame(height: 24)
                    .overlay(Capsule().strokeBorder(style: StrokeStyle(lineWidth: 1, dash: [3, 3])).foregroundStyle(.tertiary))
            }
        }
        .padding(12)
        .frame(width: 270, alignment: .leading)
        .background(BubbleShape().fill(Theme.raised(scheme)))
        .overlay(BubbleShape().stroke(Theme.hairline(scheme), lineWidth: 1))
        .shadow(color: .black.opacity(scheme == .dark ? 0.45 : 0.14), radius: 12, y: 4)
    }
}

/// A rounded card with a short tail at the bottom right, toward the pet.
struct BubbleShape: Shape {
    func path(in rect: CGRect) -> Path {
        let r: CGFloat = 14
        let body = CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: rect.height)
        var p = Path(roundedRect: body, cornerRadius: r, style: .continuous)
        let tipX = body.maxX - 34
        p.move(to: CGPoint(x: tipX - 10, y: body.maxY - 0.5))
        p.addQuadCurve(to: CGPoint(x: tipX + 6, y: body.maxY + 11), control: CGPoint(x: tipX + 2, y: body.maxY + 2))
        p.addQuadCurve(to: CGPoint(x: tipX + 6, y: body.maxY - 0.5), control: CGPoint(x: tipX + 4, y: body.maxY + 4))
        p.closeSubpath()
        return p
    }
}

/// A rounded chip for the 005 voice entry: the pet is listening.
private struct ListeningChip: View {
    @Environment(\.colorScheme) private var scheme
    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "mic.fill").font(.system(size: 11)).foregroundStyle(Theme.brand)
            HStack(spacing: 2) {
                ForEach([5.0, 9, 13, 8, 11, 6, 9, 4], id: \.self) { h in
                    Capsule().fill(Theme.brand.opacity(0.8)).frame(width: 2.5, height: h)
                }
            }
            Text(L.t("Listening… (later, 005)", "在听……（以后，005）")).font(.system(size: 11)).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10).frame(height: 26)
        .background(Capsule().fill(Theme.raised(scheme)))
        .overlay(Capsule().strokeBorder(style: StrokeStyle(lineWidth: 1, dash: [3, 3])).foregroundStyle(.tertiary))
    }
}

// MARK: - Sheets

private struct Desktop: View {
    let dark: Bool
    var body: some View {
        LinearGradient(
            colors: dark
                ? [Color(red: 0.12, green: 0.15, blue: 0.22), Color(red: 0.20, green: 0.17, blue: 0.24)]
                : [Color(red: 0.66, green: 0.76, blue: 0.86), Color(red: 0.86, green: 0.82, blue: 0.78)],
            startPoint: .topLeading, endPoint: .bottomTrailing
        )
    }
}

private struct SheetTitle: View {
    let title: String
    let subtitle: String
    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title).font(.system(size: 18, weight: .semibold))
            Text(subtitle).font(.system(size: 12)).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct Tile<Content: View>: View {
    let dark: Bool
    @ViewBuilder let content: () -> Content
    var body: some View {
        content()
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Desktop(dark: dark))
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}

struct DirectionsSheet: View {
    let dark: Bool
    private let poses: [PetPose] = [.idle, .working, .needsYou, .interrupted, .happy]

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            SheetTitle(
                title: L.t("Desktop pet · three directions", "桌面宠物 · 三个方向"),
                subtitle: L.t("All drawn from the Continuum loop; no outside artwork.", "全部由 Continuum 无限环自绘，没有外来素材。")
            )
            HStack(spacing: 0) {
                Spacer().frame(width: 190)
                ForEach(poses, id: \.self) { pose in
                    Text(pose.title).font(.system(size: 12, weight: .medium)).foregroundStyle(.secondary)
                        .frame(width: 140)
                }
            }
            ForEach(PetDirection.allCases, id: \.self) { dir in
                HStack(spacing: 0) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(dir.title).font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(dir == .pebble ? Theme.brand : .primary)
                        Text(dir.blurb).font(.system(size: 11)).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .frame(width: 176, alignment: .leading)
                    .padding(.trailing, 14)
                    ForEach(poses, id: \.self) { pose in
                        Tile(dark: dark) { pet(dir, pose) }
                            .frame(width: 132, height: 132)
                            .padding(.horizontal, 4)
                    }
                }
            }
        }
        .padding(24)
        .frame(width: 940)
        .background(Theme.surface(dark ? .dark : .light).opacity(1))
    }

    @ViewBuilder private func pet(_ dir: PetDirection, _ pose: PetPose) -> some View {
        switch dir {
        case .ribbon: RibbonPet(pose: pose, size: 84)
        case .pebble: PebblePet(pose: pose, size: 84)
        case .orbit: OrbitPet(pose: pose, size: 84)
        }
    }
}

struct PosesSheet: View {
    let dark: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            SheetTitle(
                title: L.t("Mo · six poses", "小莫 · 六个动作"),
                subtitle: L.t("The antenna light is the status lamp; the loop is the face.", "天线上的灯是状态灯；无限环是脸。")
            )
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(200), spacing: 12), count: 3), spacing: 12) {
                ForEach(PetPose.allCases, id: \.self) { pose in
                    VStack(spacing: 8) {
                        Tile(dark: dark) { PebblePet(pose: pose, size: 104) }
                            .frame(width: 200, height: 170)
                        Text(pose.title).font(.system(size: 13, weight: .semibold))
                        Text(pose.caption).font(.system(size: 11)).foregroundStyle(.secondary)
                            .multilineTextAlignment(.center).frame(height: 28, alignment: .top)
                    }
                }
            }
        }
        .padding(24)
        .frame(width: 672)
        .background(Theme.surface(dark ? .dark : .light).opacity(1))
    }
}

struct FramesSheet: View {
    let dark: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            SheetTitle(
                title: L.t("Motion keyframes", "动作关键帧"),
                subtitle: L.t("Parametric: one shape, a few numbers per frame. No sprite sheets.", "参数化：同一个形状，每帧只改几个数。不用帧图。")
            )
            row(L.t("Working: gap phase 0 → 1 over 1.6 s, linear, loops", "在忙：缺口相位 0 → 1，1.6 秒，匀速循环"),
                (0..<8).map { i in AnyView(workingFrame(Double(i) / 8)) })
            row(L.t("Idle: blink, 0.18 s (lobes squash to 0.1 and back)", "待机眨眼：0.18 秒（两瓣压扁到 0.1 再回来）"),
                [1.0, 0.6, 0.15, 0.6, 1.0].map { s in AnyView(blinkFrame(s)) })
            row(L.t("Needs you: two hops, spring (response 0.35, damping 0.55), then still", "需要你：跳两下，弹簧（0.35 / 0.55），然后停住"),
                [0.0, -0.06, -0.10, -0.03, 0.0, -0.07, 0.0].map { h in AnyView(hopFrame(h)) })
        }
        .padding(24)
        .frame(width: 940)
        .background(Theme.surface(dark ? .dark : .light).opacity(1))
    }

    private func row(_ label: String, _ frames: [AnyView]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(.system(size: 12, weight: .medium)).foregroundStyle(.secondary)
            HStack(spacing: 6) {
                ForEach(Array(frames.enumerated()), id: \.offset) { _, f in
                    Tile(dark: dark) { f }.frame(width: 104, height: 100)
                }
            }
        }
    }

    private func face(_ f: LoopFace, eye: Color = Theme.brand, hop: CGFloat = 0) -> some View {
        let size: CGFloat = 64
        let body = size * 0.78
        return ZStack {
            RoundedRectangle(cornerRadius: body * 0.40, style: .continuous)
                .fill(LinearGradient(colors: [graphiteTop, graphiteBottom], startPoint: .top, endPoint: .bottom))
                .frame(width: body, height: body * 0.92)
            f.stroke(eye, style: StrokeStyle(lineWidth: size * 0.065, lineCap: .round, lineJoin: .round))
                .frame(width: body * 0.76, height: body * 0.76)
                .offset(y: -body * 0.03)
        }
        .offset(y: hop * size)
    }

    private func workingFrame(_ phase: Double) -> some View { face(LoopFace(phase: phase)) }
    private func blinkFrame(_ s: Double) -> some View { face(LoopFace(left: s, right: s)) }
    private func hopFrame(_ h: CGFloat) -> some View {
        face(LoopFace(amp: 15.5, left: 1.1, right: 1.1, gapStart: 0.72, gapEnd: 0.78), hop: h)
    }
}

struct BubblesSheet: View {
    let dark: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            SheetTitle(
                title: L.t("Speech bubbles", "气泡"),
                subtitle: L.t("Short, plain, a little warm. Dashed slots are reserved for 004 and 005.", "短、直白、带点温度。虚线位置留给 004 与 005。")
            )
            HStack(spacing: 12) {
                scene(
                    PetBubble(kind: .needsYou,
                              title: L.t("api-review needs you", "api-review 需要你"),
                              message: L.t("Claude wants to run a command and is waiting for your nod.", "Claude 想运行一条命令，在等你点头。"),
                              actions: [L.t("Open", "去看看"), L.t("Later", "稍后")],
                              speaking: true,
                              reserved: L.t("Allow / Deny here (004)", "在这里同意 / 拒绝（004）")),
                    pose: .needsYou)
                scene(
                    PetBubble(kind: .interrupted,
                              title: L.t("fix-invoice dropped", "fix-invoice 断了"),
                              message: L.t("Nothing is lost. One click brings it back.", "别慌，进度都在。点一下就找回来。"),
                              actions: [L.t("Bring back", "找回"), L.t("Not now", "先不")]),
                    pose: .interrupted)
            }
            HStack(spacing: 12) {
                scene(
                    PetBubble(kind: .restored,
                              title: L.t("All 3 sessions are back", "3 个会话都找回来了"),
                              message: L.t("Civilizations carry on. So do conversations.", "文明会延续，对话也是。"),
                              speaking: true),
                    pose: .happy)
                scene(
                    PetBubble(kind: .merged,
                              title: L.t("2 sessions are waiting", "2 个会话在等你"),
                              message: L.t("term-kit and billing finished their turns.", "term-kit 和 billing 这一轮都做完了。"),
                              actions: [L.t("Show", "看看")]),
                    pose: .idle, listening: true)
            }
        }
        .padding(24)
        .frame(width: 940)
        .background(Theme.surface(dark ? .dark : .light).opacity(1))
    }

    private func scene(_ bubble: PetBubble, pose: PetPose, listening: Bool = false) -> some View {
        Tile(dark: dark) {
            ZStack(alignment: .bottomTrailing) {
                Color.clear
                VStack(alignment: .trailing, spacing: 10) {
                    bubble
                    HStack(alignment: .bottom, spacing: 8) {
                        if listening { ListeningChip().padding(.bottom, 20) }
                        PebblePet(pose: pose, size: 72)
                    }
                }
                .padding(.trailing, 18)
                .padding(.bottom, 6)
            }
        }
        .frame(width: 440, height: 270)
    }
}

// MARK: - Rendering

enum PetSheets {
    @MainActor static func run(outDir: String) {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)
        func out(_ name: String) -> String { (outDir as NSString).appendingPathComponent(name) }

        // Sheets come in zh/light and en/dark (the T1.1 convention); bubbles,
        // the part people read, in all four combinations.
        for (lang, dark) in [(Lang.zh, false), (Lang.en, true), (Lang.zh, true), (Lang.en, false)] {
            L.lang = lang
            let tag = "\(lang == .zh ? "zh" : "en")-\(dark ? "dark" : "light")"
            snapshot(BubblesSheet(dark: dark), dark: dark, to: out("bubbles-\(tag).png"))
            guard (lang == .zh) != dark else { continue }
            snapshot(DirectionsSheet(dark: dark), dark: dark, to: out("directions-\(tag).png"))
            snapshot(PosesSheet(dark: dark), dark: dark, to: out("poses-\(tag).png"))
            snapshot(FramesSheet(dark: dark), dark: dark, to: out("frames-\(tag).png"))
        }
        exit(0)
    }

    @MainActor private static func snapshot<V: View>(_ view: V, dark: Bool, to path: String) {
        let host = NSHostingView(rootView: view.environment(\.colorScheme, dark ? .dark : .light))
        let size = host.fittingSize
        host.frame = NSRect(origin: .zero, size: size)
        let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.4))
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { return }
        host.cacheDisplay(in: host.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
    }
}
