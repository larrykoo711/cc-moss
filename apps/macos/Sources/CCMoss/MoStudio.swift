import MossKit
import SwiftUI

/// The top of Settings › Mo: Mo itself on a little stage on the left, the
/// switch and the size on the right. They play together: Mo wakes or dozes
/// with the switch, grows and shrinks with the size and says so in a
/// bubble, and a click on Mo gets a hop and a word.
struct MoStudio: View {
    @Binding var enabled: Bool
    @Binding var size: Double
    var changed: () -> Void

    @State private var bubble: String?
    @State private var bubbleID = 0
    @State private var hop = false
    @State private var phrases = Catchphrases()
    @State private var start = Date()
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 0) {
            stage
                .frame(width: 230, height: 214)
            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(L.t("Mo", "小莫")).font(.system(size: 22, weight: .semibold))
                    Text(enabled
                         ? L.t("On your desktop, keeping an eye on things.", "在桌面上，替你看着呢。")
                         : L.t("Asleep. Switch it on and it moves in.", "睡着了。打开就搬到你桌面上。"))
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                HStack(spacing: 10) {
                    Text(L.t("Show Mo on desktop", "住在桌面上")).font(.system(size: 14))
                    Spacer()
                    Toggle("", isOn: $enabled.animation(.easeOut(duration: 0.2)))
                        .toggleStyle(.switch)
                        .handCursor()
                        .labelsHidden()
                        .controlSize(.small)
                        .onChange(of: enabled) { _, on in
                            changed()
                            say(on ? L.t("I'm here!", "我来啦！") : L.t("zzz…", "那我先睡会儿…"))
                        }
                }
                if enabled {
                    HStack(spacing: 10) {
                        Text(L.t("Size", "大小")).font(.system(size: 14)).fixedSize()
                        Spacer()
                        SlidingPicker(selection: $size, options: [
                            (48, L.t("Small", "小")),
                            (64, L.t("Medium", "中")),
                            (88, L.t("Large", "大")),
                        ])
                        .onChange(of: size) { old, new in
                            changed()
                            say(new > old ? L.t("Bigger!", "变大啦！") : L.t("A bit smaller", "缩小一点～"))
                        }
                    }
                    .transition(.opacity)
                }
                Text(L.t("Tip: click Mo.", "小提示：点点小莫。"))
                    .font(.system(size: 11))
                    .foregroundStyle(Theme.caption)
            }
            .padding(.horizontal, 20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Color.primary.opacity(0.025)))
    }

    // MARK: Stage

    private var stage: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(LinearGradient(colors: stageColors, startPoint: .topLeading, endPoint: .bottomTrailing))
            TimelineView(.animation(paused: reduceMotion || !enabled)) { context in
                let t = context.date.timeIntervalSince(start)
                let bob = reduceMotion || !enabled ? 0 : sin(t * 1.6) * 3
                ZStack {
                    // Drawn once at the largest size, scaled to the chosen one.
                    LayerPet(pose: enabled ? .happy : .sleeping, size: 88, dark: scheme == .dark)
                        .scaleEffect(enabled ? CGFloat(size) / 88 : 0.73)
                        .offset(y: bob + (hop ? -14 : 0))
                        .animation(.spring(response: 0.35, dampingFraction: 0.55), value: size)
                        .animation(.spring(response: 0.25, dampingFraction: 0.45), value: hop)
                        .contentShape(Circle())
                        .onTapGesture { poke() }
                        .handCursor()
                        .accessibilityLabel(L.t("Mo", "小莫"))
                        .accessibilityAddTraits(.isButton)
                    if let bubble {
                        Text(bubble)
                            .font(.system(size: 12, weight: .medium))
                            .padding(.horizontal, 10)
                            .frame(height: 26)
                            .background(Capsule().fill(Theme.raised(scheme)))
                            .shadow(color: .black.opacity(0.12), radius: 6, y: 2)
                            .offset(y: -78)
                            .id(bubbleID)
                            .transition(.scale(scale: 0.6, anchor: .bottom).combined(with: .opacity))
                    }
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }

    private var stageColors: [Color] {
        if scheme == .dark {
            return enabled ? [Color(red: 0.16, green: 0.24, blue: 0.22), Color(red: 0.12, green: 0.16, blue: 0.16)]
                           : [Color(red: 0.14, green: 0.15, blue: 0.20), Color(red: 0.10, green: 0.11, blue: 0.14)]
        }
        return enabled ? [Color(red: 0.84, green: 0.96, blue: 0.92), Color(red: 0.93, green: 0.98, blue: 0.96)]
                       : [Color(red: 0.88, green: 0.89, blue: 0.95), Color(red: 0.94, green: 0.95, blue: 0.98)]
    }

    // MARK: Play

    private func poke() {
        guard enabled else {
            say(L.t("Mm… switch me on first", "嗯…先把我打开嘛"))
            return
        }
        hop = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) { hop = false }
        say(phrases.next())
    }

    private func say(_ text: String) {
        bubbleID += 1
        let mine = bubbleID
        withAnimation(.spring(response: 0.3, dampingFraction: 0.65)) { bubble = text }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.8) {
            guard bubbleID == mine else { return }
            withAnimation(.easeOut(duration: 0.25)) { bubble = nil }
        }
    }
}
