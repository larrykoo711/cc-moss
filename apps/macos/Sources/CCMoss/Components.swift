import AppKit
import MossKit
import SwiftUI

/// The state filter: a segmented control whose white thumb slides to the
/// chosen segment. Each segment carries its count (iStat Menus' summary,
/// Raycast's polish).
struct StateFilterBar: View {
    let counts: StateCounts
    @Binding var filter: SessionState?
    @State private var hovered: Int?
    @Environment(\.colorScheme) private var scheme

    private struct Segment {
        let state: SessionState?
        let title: String
        let count: Int?
    }

    private var segments: [Segment] {
        [
            Segment(state: nil, title: L.t("All", "全部"), count: nil),
            Segment(state: .working, title: SessionState.working.shortLabel, count: counts.working),
            Segment(state: .waiting, title: SessionState.waiting.shortLabel, count: counts.waiting),
            Segment(state: .interrupted, title: SessionState.interrupted.shortLabel, count: counts.interrupted),
        ]
    }

    private var selectedIndex: Int {
        segments.firstIndex { $0.state == filter } ?? 0
    }

    /// Natural width of each segment: icon, count, padding, and the title
    /// only for the selected one (the others are icon + count).
    private var naturalWidths: [CGFloat] {
        let title = NSFont.systemFont(ofSize: 12, weight: .medium)
        let count = NSFont.systemFont(ofSize: 11, weight: .semibold)
        return segments.enumerated().map { i, seg in
            var w: CGFloat = 14
            if i == selectedIndex || seg.count == nil {
                w += 5 + (seg.title as NSString).size(withAttributes: [.font: title]).width
            }
            if let n = seg.count {
                w += 5 + ("\(n)" as NSString).size(withAttributes: [.font: count]).width
            }
            return ceil(w) + 22
        }
    }

    /// Each segment gets its natural width plus an equal share of what is
    /// left, so long labels ("Interrupted") never touch the edge.
    private func widths(total: CGFloat) -> [CGFloat] {
        let natural = naturalWidths
        let extra = max(0, total - natural.reduce(0, +)) / CGFloat(natural.count)
        return natural.map { $0 + extra }
    }

    var body: some View {
        // One thumb that always exists and only moves: a real slide, never a
        // cross-fade between two thumbs.
        GeometryReader { geo in
            let w = widths(total: geo.size.width)
            let x = w.prefix(selectedIndex).reduce(0, +)
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 7)
                    .fill(Theme.raised(scheme))
                    .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Theme.hairline(scheme), lineWidth: 0.5))
                    .shadow(color: .black.opacity(scheme == .dark ? 0.4 : 0.10), radius: 2, y: 1)
                    .frame(width: w[selectedIndex], height: geo.size.height)
                    .offset(x: x)
                    .animation(.spring(response: 0.32, dampingFraction: 0.86), value: selectedIndex)
                // Icons, labels and counts go straight to their new places;
                // only the thumb travels. Animating both made them cross paths.
                HStack(spacing: 0) {
                    ForEach(Array(segments.enumerated()), id: \.offset) { i, seg in
                        segment(i, seg).frame(width: w[i], height: geo.size.height)
                    }
                }
                .transaction { $0.animation = nil }
            }
        }
        .frame(height: 26)
        .padding(3)
        .background(
            RoundedRectangle(cornerRadius: 9)
                .fill(scheme == .dark ? Color.white.opacity(0.07) : Color.black.opacity(0.055))
        )
    }

    private func segment(_ i: Int, _ seg: Segment) -> some View {
        let on = i == selectedIndex
        let empty = seg.count == 0
        return Button {
            filter = seg.state
        } label: {
            HStack(spacing: 5) {
                Image(systemName: seg.state.map(Icons.state) ?? Icons.all)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(seg.state.map(Theme.color) ?? Color.secondary)
                    .opacity(empty ? 0.4 : 1)
                // Only the chosen segment spells out its name; the rest are
                // icon and count, named by their tooltip.
                if on || seg.count == nil {
                    Text(seg.title)
                        .font(.system(size: 12, weight: .medium))
                        .lineLimit(1)
                        .fixedSize()
                        // No fade: a fading label would overlap its count
                        // while the segment narrows. Only widths animate.
                        .transition(.identity)
                }
                if let count = seg.count {
                    Text("\(count)")
                        .font(.system(size: 11, weight: .semibold, design: .rounded))
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }
            }
            .foregroundStyle(on || hovered == i ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
            .opacity(empty && !on ? 0.6 : 1)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .handCursor()
        .focusable(false)
        .tip(seg.title)
        .accessibilityLabel(seg.count.map { "\(seg.title) \($0)" } ?? seg.title)
        .onHover { hovered = $0 ? i : (hovered == i ? nil : hovered) }
    }

    /// Tab moves to the next segment, ⇧Tab to the previous one.
    static func cycle(_ current: SessionState?, forward: Bool) -> SessionState? {
        let order: [SessionState?] = [nil, .working, .waiting, .interrupted]
        let i = order.firstIndex { $0 == current } ?? 0
        return order[(i + (forward ? 1 : order.count - 1)) % order.count]
    }
}

struct SectionHeader: View {
    let project: Project

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: Icons.project)
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(.tertiary)
            Text(project.label)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.secondary)
                .lineLimit(1)
            if project.worktree != nil {
                Image(systemName: Icons.worktree)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .tip(L.t("A git worktree of \((project.worktree!.repo as NSString).lastPathComponent)",
                              "\((project.worktree!.repo as NSString).lastPathComponent) 的 git worktree"))
                    .accessibilityLabel("git worktree")
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 12)
        .padding(.bottom, 4)
        .tip(project.path)
    }
}

/// One session in two lines: state, name, when / your note or its last message.
struct SessionRow: View {
    let session: Session
    let selected: Bool
    /// The row's main action, shown as a button while the row is hovered.
    var action: PanelAction?
    var perform: (PanelAction) -> Void = { _ in }
    /// Why it could not be brought back; replaces the second line.
    var problem: String?
    /// On its way back right now.
    var restoring = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            stateIcon
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(session.title)
                        .font(.system(size: 13, weight: .medium))
                        .lineLimit(1)
                    if !session.inTmux {
                        Image(systemName: Icons.outsideTmux)
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(.tertiary)
                            .tip(L.t("Not in tmux: started outside tmux, so it can't be opened from here.",
                                      "不在 tmux 里：它不是在 tmux 里开的，没法从这里打开。"))
                            .accessibilityLabel(L.t("not in tmux", "不在 tmux 里"))
                    }
                }
                if let problem {
                    HStack(spacing: 4) {
                        Image(systemName: Icons.cannotRestore)
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(Theme.amber)
                        Text(problem).lineLimit(1).truncationMode(.tail)
                    }
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .tip(problem)
                } else if let sub = session.subtitle {
                    HStack(spacing: 4) {
                        Image(systemName: session.subtitleIsNote ? Icons.note : Icons.lastPrompt)
                            .font(.system(size: 10))
                            .foregroundStyle(.tertiary)
                        Text(sub).lineLimit(1).truncationMode(.tail)
                    }
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 8)
            ZStack(alignment: .trailing) {
                Text(L.ago(session.lastActiveAt))
                    .font(.system(size: 11))
                    .foregroundStyle(.tertiary)
                    .opacity(action == nil && !restoring ? 1 : 0)
                if restoring {
                    ProgressView().controlSize(.small).scaleEffect(0.7)
                        .frame(width: 30, height: 30)
                        .accessibilityLabel(L.t("Bringing it back", "正在找回"))
                } else if let action {
                    RowActionButton(action: action, perform: perform)
                        .transition(.opacity.combined(with: .scale(scale: 0.9)))
                }
            }
            .animation(.easeOut(duration: 0.12), value: action == nil)
        }
        .padding(.horizontal, 10)
        .frame(height: 48)
        .background(
            RoundedRectangle(cornerRadius: 8)
                .fill(selected ? Theme.selection(scheme) : .clear)
        )
    }

    /// Raycast-style icon tile: the state's symbol on a tint of its colour.
    private var stateIcon: some View {
        // A session asking you something trumps what it is doing.
        let color = session.needsYou ? Theme.brand : Theme.color(session.state)
        let label = session.needsYou ? L.t("Needs you", "需要你确认") : session.state.label
        return ZStack {
            RoundedRectangle(cornerRadius: 6)
                .fill(color.opacity(scheme == .dark ? 0.22 : 0.14))
            if session.needsYou {
                Image(systemName: Icons.needsYou)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(color)
            } else if session.state == .working {
                WorkingRing()
            } else {
                Image(systemName: Icons.state(session.state))
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(color)
            }
        }
        .frame(width: 24, height: 24)
        .tip(label)
        .accessibilityLabel(label)
    }
}

/// A small turning arc in the brand colour: Claude is on it.
struct WorkingRing: View {
    @State private var turning = false

    var body: some View {
        Circle()
            .trim(from: 0, to: 0.72)
            .stroke(Theme.brand, style: StrokeStyle(lineWidth: 1.8, lineCap: .round))
            .frame(width: 11, height: 11)
            .rotationEffect(.degrees(turning ? 360 : 0))
            .animation(.linear(duration: 1.1).repeatForever(autoreverses: false), value: turning)
            .onAppear { turning = true }
    }
}

/// What to show instead of the list: not installed, an error, nothing yet.
struct GuideCard: View {
    let symbol: String
    let title: String
    let message: String
    let command: String
    let copy: (String) -> Void

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: symbol)
                .font(.system(size: 28, weight: .light))
                .foregroundStyle(.secondary)
            Text(title).font(.system(size: 14, weight: .semibold))
            Text(message)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            Button { copy(command) } label: {
                HStack(spacing: 8) {
                    Text(command)
                        .font(.system(size: 12, design: .monospaced))
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                    Image(systemName: "doc.on.doc").font(.system(size: 11))
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .background(RoundedRectangle(cornerRadius: 7).fill(Color.primary.opacity(0.06)))
            }
            .buttonStyle(.plain)
        .handCursor()
        .focusable(false)
            .tip(L.t("Copy", "复制"))
        }
        .padding(28)
    }
}

/// The ⌘K panel: every action for the selected session, with its shortcut.
struct ActionsMenu: View {
    let actions: [PanelAction]
    let close: () -> Void
    @State private var hovered: UUID?
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(actions) { a in
                Button {
                    close()
                    a.perform()
                } label: {
                    HStack(spacing: 9) {
                        ActionIcon(action: a, symbolSize: 12).foregroundStyle(.secondary)
                        Text(a.title).font(.system(size: 13)).lineLimit(1).fixedSize()
                        Spacer(minLength: 16)
                        if !a.shortcut.isEmpty { KeyCap(text: a.shortcut) }
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 30)
                    .background(RoundedRectangle(cornerRadius: 6).fill(hovered == a.id ? Theme.selection(scheme) : .clear))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
        .handCursor()
        .focusable(false)
                .onHover { if $0 { hovered = a.id } }
            }
        }
        .padding(6)
        .frame(width: 280)
        .background(Theme.raised(scheme), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.hairline(scheme)))
        .shadow(color: .black.opacity(0.18), radius: 14, y: 6)
    }
}

/// A small segmented control in the panel's style: one white thumb slides
/// to the chosen option (used in settings instead of the system picker).
struct SlidingPicker<Value: Hashable>: View {
    @Binding var selection: Value
    let options: [(Value, String)]
    @Environment(\.colorScheme) private var scheme

    private var index: Int { options.firstIndex { $0.0 == selection } ?? 0 }

    private var widths: [CGFloat] {
        let font = NSFont.systemFont(ofSize: 12, weight: .medium)
        return options.map { ceil(($0.1 as NSString).size(withAttributes: [.font: font]).width) + 24 }
    }

    var body: some View {
        let w = widths
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: 6)
                .fill(Theme.raised(scheme))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Theme.hairline(scheme), lineWidth: 0.5))
                .shadow(color: .black.opacity(scheme == .dark ? 0.4 : 0.10), radius: 1.5, y: 1)
                .frame(width: w[index], height: 24)
                .offset(x: w.prefix(index).reduce(0, +))
                .animation(.spring(response: 0.3, dampingFraction: 0.86), value: index)
            HStack(spacing: 0) {
                ForEach(Array(options.enumerated()), id: \.offset) { i, option in
                    Button { selection = option.0 } label: {
                        Text(option.1)
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(i == index ? .primary : .secondary)
                            .frame(width: w[i], height: 24)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(PressStyle())
                    .focusable(false)
                }
            }
            .transaction { $0.animation = nil }
        }
        .padding(2)
        .background(RoundedRectangle(cornerRadius: 8).fill(scheme == .dark ? Color.white.opacity(0.07) : Color.black.opacity(0.055)))
        .fixedSize()
    }
}

/// The small button at the end of a hovered session row: the chosen
/// terminal's icon to open it, or the action that fits (bring back, copy).
struct RowActionButton: View {
    let action: PanelAction
    let perform: (PanelAction) -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button { perform(action) } label: {
            Group {
                if let image = action.image {
                    Image(nsImage: image).resizable().interpolation(.high).frame(width: 18, height: 18)
                } else {
                    Image(systemName: action.icon).font(.system(size: 13, weight: .medium))
                }
            }
            .frame(width: 30, height: 30)
            .background(RoundedRectangle(cornerRadius: 8).fill(hovering ? Theme.raised(scheme) : Color.primary.opacity(0.06)))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(hovering ? Theme.hairline(scheme) : .clear))
            .shadow(color: .black.opacity(hovering ? 0.08 : 0), radius: 2, y: 1)
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hovering = inside } }
        .tip(action.help ?? action.title)
        .accessibilityLabel(action.title)
    }
}
