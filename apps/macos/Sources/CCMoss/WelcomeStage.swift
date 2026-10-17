import AppKit
import MossKit
import SwiftUI

/// The right half of the welcome: one small, quietly moving scene per step,
/// drawn from the app's own pieces (Mo, the icon, keycaps, a notification).
/// Still under Reduce Motion.
struct WelcomeStage: View {
    let step: WelcomeFlow.Step
    let dark: Bool
    /// The shortcut was just pressed on its step.
    var tried = false
    /// The terminal is connected.
    var ready = false
    /// Screenshots: one moment, every entrance done.
    var still: Double?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var start = Date()
    @AppStorage(NewSession.workspaceKey) private var workspace = ""
    /// Control presses before this step, so only yours count.
    @State private var keysAtStart = 0

    var body: some View {
        TimelineView(.animation(paused: reduceMotion || still != nil)) { context in
            // Under Reduce Motion: a moment after every entrance, held still.
            let t = still ?? (reduceMotion ? 2.0 : context.date.timeIntervalSince(start))
            scene(t)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder private func scene(_ t: Double) -> some View {
        switch step {
        case .welcome: welcome(t)
        case .tools: tools(t)
        case .workspace: workspace(t)
        case .shortcut: shortcut(t)
        case .startup: startup(t)
        case .mo: mo(t)
        case .done: done(t)
        }
    }

    // MARK: Scenes

    /// Mo, bobbing, with the three things it tells you floating round it.
    private func welcome(_ t: Double) -> some View {
        ZStack {
            LayerPet(pose: .happy, size: 120, dark: dark)
                .offset(y: sin(t * 1.6) * 5)
            chip(L.t("Busy", "在忙"), icon: "waveform", tint: Theme.brand)
                .offset(x: -110, y: -86 + sin(t * 1.3) * 4)
                .entrance(t, after: 0.3)
            chip(L.t("Waiting for you", "在等你"), icon: "bubble.left.fill", tint: Theme.amber)
                .offset(x: 104, y: -40 + sin(t * 1.1 + 1) * 4)
                .entrance(t, after: 0.55)
            chip(L.t("Stopped", "断了"), icon: "arrow.counterclockwise", tint: Theme.red)
                .offset(x: -86, y: 96 + sin(t * 1.2 + 2) * 4)
                .entrance(t, after: 0.8)
        }
    }

    /// The terminal and cc-moss, joined by a line that dots travel along.
    private func tools(_ t: Double) -> some View {
        HStack(spacing: 0) {
            IconTile(symbol: "terminal", colors: [Color(white: 0.30), Color(white: 0.12)], size: 74)
                .shadow(color: .black.opacity(0.15), radius: 10, y: 4)
            ZStack {
                Capsule().fill(Color.primary.opacity(0.1)).frame(height: 3)
                GeometryReader { g in
                    ForEach(0..<3, id: \.self) { i in
                        let p = ((t * 0.6 + Double(i) / 3).truncatingRemainder(dividingBy: 1))
                        Circle()
                            .fill(ready ? Color.green : Theme.brand)
                            .frame(width: 7, height: 7)
                            .position(x: g.size.width * p, y: g.size.height / 2)
                            .opacity(sin(p * .pi))
                    }
                }
            }
            .frame(width: 110, height: 12)
            ZStack(alignment: .topTrailing) {
                AppIconTile(size: 74)
                if ready {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 24))
                        .foregroundStyle(.white, Color.green)
                        .offset(x: 8, y: -8)
                        .transition(.scale.combined(with: .opacity))
                }
            }
            .animation(.spring(response: 0.35, dampingFraction: 0.55), value: ready)
        }
    }

    /// Your folder, named, with its full path under it and sessions
    /// settling into it one by one.
    private func workspace(_ t: Double) -> some View {
        ZStack {
            VStack(spacing: 8) {
                Spacer().frame(height: 200)
                Text(workspace.isEmpty ? L.t("The folder you used last", "上次用的目录") : (workspace as NSString).lastPathComponent)
                    .font(.system(size: 15, weight: .semibold))
                if !workspace.isEmpty {
                    Text(Grouping.tilde(workspace, home: NSHomeDirectory()))
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                        .lineLimit(3)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(Capsule().fill(Theme.raised(dark ? .dark : .light).opacity(0.8)))
                        .frame(maxWidth: 300)
                        .transition(.opacity)
                }
            }
            .animation(.spring(response: 0.35, dampingFraction: 0.8), value: workspace)
            Image(systemName: "folder.fill")
                .font(.system(size: 120))
                .foregroundStyle(LinearGradient(colors: [Color(red: 1.0, green: 0.78, blue: 0.35), Color(red: 0.96, green: 0.62, blue: 0.20)],
                                                startPoint: .top, endPoint: .bottom))
                .shadow(color: .black.opacity(0.12), radius: 12, y: 6)
            ForEach(0..<3, id: \.self) { i in
                let cycle = 3.0
                let p = ((t + Double(i) * cycle / 3) .truncatingRemainder(dividingBy: cycle)) / cycle
                chip(["acme-web", "api-review", "billing-fix"][i], icon: "bubble.left.fill", tint: Theme.brand)
                    .scaleEffect(1 - p * 0.5)
                    .offset(x: CGFloat(i - 1) * 60 * (1 - p), y: -150 + p * 150)
                    .opacity(p < 0.85 ? 1 : (1 - p) / 0.15)
            }
        }
    }

    /// Two Control keys. They demo the double tap until you try it, then
    /// follow your real presses (first press the left one, second the
    /// right); when it works they rise and a little panel slides up, just
    /// as the real one will.
    private func shortcut(_ t: Double) -> some View {
        let keys = HotKeyCenter.shared.keys
        let count = keys.presses - keysAtStart
        let yours = still == nil && (count > 0 || keys.controlDown)
        let phase = t.truncatingRemainder(dividingBy: 2.2)
        func demo(_ at: Double) -> Bool { !yours && !tried && phase > at && phase < at + 0.18 }
        let left = demo(0.4) || (yours && keys.controlDown && count % 2 == 0)
        let right = demo(0.75) || (yours && keys.controlDown && count % 2 == 1)
        return ZStack {
            if tried { burst(t).offset(y: -60) }
            HStack(spacing: 18) {
                keycap("⌃", pressed: left)
                keycap("⌃", pressed: right)
            }
            .scaleEffect(tried ? 0.72 : 1)
            .offset(y: tried ? -96 : 0)
            if tried {
                miniPanel
                    .offset(y: 56)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            } else if yours {
                Text(count % 2 == 1 ? L.t("Once more!", "再按一下！") : L.t("Tap ⌃ twice", "连按两下 ⌃"))
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(.secondary)
                    .offset(y: 84)
            }
        }
        .animation(.spring(response: 0.45, dampingFraction: 0.78), value: tried)
        .onAppear { keysAtStart = HotKeyCenter.shared.keys.presses }
    }

    /// The panel, in miniature: what the shortcut opens.
    private var miniPanel: some View {
        let s: ColorScheme = dark ? .dark : .light
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass").font(.system(size: 10))
                Text(L.t("Search sessions…", "搜索会话…")).font(.system(size: 11))
                Spacer()
            }
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .frame(height: 28)
            Rectangle().fill(Color.primary.opacity(0.06)).frame(height: 1)
            ForEach(Array([("acme-web", Theme.brand, L.t("Busy", "在忙")),
                           ("api-review", Theme.amber, L.t("Waiting", "在等你")),
                           ("billing-fix", Theme.red, L.t("Stopped", "断了"))].enumerated()), id: \.offset) { i, row in
                HStack(spacing: 8) {
                    Circle().fill(row.1).frame(width: 7, height: 7)
                    Text(row.0).font(.system(size: 12, weight: .medium))
                    Spacer()
                    Text(row.2).font(.system(size: 10)).foregroundStyle(.secondary)
                }
                .padding(.horizontal, 10)
                .frame(height: 30)
                .background(i == 0 ? Theme.selection(s) : .clear)
            }
        }
        .frame(width: 230)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Theme.raised(s)))
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .shadow(color: .black.opacity(0.16), radius: 14, y: 6)
    }

    /// A menu bar with the cc-moss icon, and a notification sliding in.
    private func startup(_ t: Double) -> some View {
        let cycle = 4.0
        let p = t.truncatingRemainder(dividingBy: cycle)
        let shown = p > 0.6 && p < 3.4
        return VStack(spacing: 0) {
            HStack(spacing: 10) {
                Spacer()
                Continuum(phase: (t / 2).truncatingRemainder(dividingBy: 1))
                    .stroke(style: StrokeStyle(lineWidth: 2, lineCap: .round))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 16, height: 16)
                Image(systemName: "wifi").font(.system(size: 11))
                Image(systemName: "battery.75percent").font(.system(size: 13))
                Text("9:41").font(.system(size: 12, weight: .medium))
            }
            .foregroundStyle(.primary.opacity(0.7))
            .padding(.horizontal, 16)
            .frame(height: 28)
            .background(.ultraThinMaterial)
            HStack {
                Spacer()
                banner
                    .offset(x: shown ? 0 : 340)
                    .animation(.spring(response: 0.5, dampingFraction: 0.82), value: shown)
            }
            .padding(14)
            Spacer()
        }
    }

    /// Mo in the corner of a little desktop, with a puff.
    private func mo(_ t: Double) -> some View {
        ZStack(alignment: .bottomTrailing) {
            VStack(spacing: 10) {
                window(width: 200, height: 110).offset(x: -40)
                window(width: 170, height: 90).offset(x: 20)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            VStack(alignment: .trailing, spacing: 4) {
                Text(L.t("Psst, one needs you", "嘿，有个会话在等你"))
                    .font(.system(size: 12, weight: .medium))
                    .padding(.horizontal, 10)
                    .frame(height: 26)
                    .background(Capsule().fill(Theme.raised(dark ? .dark : .light)))
                    .opacity(t.truncatingRemainder(dividingBy: 4) > 1 ? 0.9 : 0)
                    .animation(.easeOut(duration: 0.3), value: t.truncatingRemainder(dividingBy: 4) > 1)
                LayerPet(pose: .needsYou, size: 70, dark: dark)
                    .offset(y: sin(t * 1.8) * 3)
            }
            .padding(18)
        }
    }

    /// Mo cheering, with a burst.
    private func done(_ t: Double) -> some View {
        ZStack {
            burst(t)
            LayerPet(pose: .happy, size: 130, dark: dark)
                .offset(y: -abs(sin(t * 2.4)) * 10)
        }
    }

    // MARK: Pieces

    private func chip(_ text: String, icon: String, tint: Color) -> some View {
        HStack(spacing: 6) {
            Image(systemName: icon).font(.system(size: 10, weight: .bold)).foregroundStyle(tint)
            Text(text).font(.system(size: 12, weight: .semibold))
        }
        .padding(.horizontal, 10)
        .frame(height: 28)
        .background(Capsule().fill(Theme.raised(dark ? .dark : .light)))
        .shadow(color: .black.opacity(0.10), radius: 6, y: 2)
    }

    /// A Mac Control key: the symbol, and its name small underneath.
    private func keycap(_ key: String, pressed: Bool) -> some View {
        VStack(spacing: 6) {
            Text(key).font(.system(size: 34, weight: .medium))
            Text("control").font(.system(size: 12, weight: .medium)).foregroundStyle(.secondary)
        }
            .frame(width: 92, height: 92)
            .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Theme.raised(dark ? .dark : .light)))
            .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous)
                .strokeBorder(pressed ? Theme.brand.opacity(0.7) : Color.primary.opacity(0.08), lineWidth: pressed ? 2 : 1))
            .shadow(color: .black.opacity(pressed ? 0.05 : 0.16), radius: pressed ? 2 : 10, y: pressed ? 1 : 6)
            .offset(y: pressed ? 4 : 0)
            .animation(.spring(response: 0.18, dampingFraction: 0.6), value: pressed)
    }

    private var banner: some View {
        HStack(spacing: 10) {
            AppIconTile(size: 30)
            VStack(alignment: .leading, spacing: 2) {
                Text(L.t("api-review needs you", "api-review 需要你确认")).font(.system(size: 12, weight: .semibold))
                Text(L.t("It wants to do something and is waiting for your OK.", "它想做一件事，等你点头。"))
                    .font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .padding(10)
        .frame(width: 280, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(.regularMaterial))
        .shadow(color: .black.opacity(0.15), radius: 10, y: 4)
    }

    private func window(width: CGFloat, height: CGFloat) -> some View {
        RoundedRectangle(cornerRadius: 10, style: .continuous)
            .fill(Theme.raised(dark ? .dark : .light).opacity(0.85))
            .frame(width: width, height: height)
            .overlay(alignment: .topLeading) {
                HStack(spacing: 4) {
                    ForEach(0..<3, id: \.self) { _ in Circle().fill(Color.primary.opacity(0.15)).frame(width: 6, height: 6) }
                }
                .padding(8)
            }
            .shadow(color: .black.opacity(0.08), radius: 6, y: 3)
    }

    /// Little dots flying out and fading, again and again.
    private func burst(_ t: Double) -> some View {
        ZStack {
            ForEach(0..<14, id: \.self) { i in
                let angle = Double(i) / 14 * 2 * .pi
                let p = ((t * 0.7 + Double(i % 3) * 0.12).truncatingRemainder(dividingBy: 1))
                Circle()
                    .fill([Theme.brand, Theme.amber, Color.green, Color(red: 0.55, green: 0.45, blue: 0.95)][i % 4])
                    .frame(width: 7, height: 7)
                    .offset(x: cos(angle) * (60 + p * 70), y: sin(angle) * (60 + p * 70))
                    .opacity(1 - p)
            }
        }
    }
}

private extension View {
    /// Fades and scales in once, `after` seconds into the scene.
    func entrance(_ t: Double, after: Double) -> some View {
        let p = min(max((t - after) / 0.35, 0), 1)
        return self.opacity(p).scaleEffect(0.85 + 0.15 * p)
    }
}
