import MossKit
import SwiftUI

enum Theme {
    static let brand = Color(red: 1.0, green: 0.353, blue: 0.235) // #FF5A3C, the icon's colour
    static let amber = Color(red: 0.90, green: 0.68, blue: 0.30)
    static let red = Color(red: 0.93, green: 0.30, blue: 0.30)
    static let panelWidth: CGFloat = 420
    static let panelHeight: CGFloat = 540

    /// Raycast-like surfaces: a near-opaque layer over the frosted glass, so
    /// the wallpaper does not tint the panel.
    static func surface(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.11, green: 0.11, blue: 0.12).opacity(0.92) : Color(red: 0.985, green: 0.985, blue: 0.99).opacity(0.95)
    }

    /// The floating action bar and the ⌘K menu sit one step above the panel.
    static func raised(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? Color(red: 0.24, green: 0.24, blue: 0.26) : .white
    }

    static func selection(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? Color.white.opacity(0.10) : Color.black.opacity(0.075)
    }

    static func hairline(_ scheme: ColorScheme) -> Color {
        scheme == .dark ? Color.white.opacity(0.08) : Color.black.opacity(0.08)
    }

    static func color(_ state: SessionState) -> Color {
        switch state {
        case .working: brand
        case .waiting: amber
        case .interrupted: red
        case .closed: .secondary
        }
    }

    static func symbol(_ state: SessionState) -> String {
        Icons.state(state)
    }
}

/// The app's icon vocabulary (SF Symbols). Every state, action and piece of
/// metadata shows one of these next to its words; nothing picks a symbol ad hoc.
enum Icons {
    static let all = "square.stack"
    static let project = "folder"
    static let worktree = "arrow.triangle.branch"
    static let note = "note.text"
    static let lastPrompt = "quote.bubble"
    static let time = "clock"
    static let outsideTmux = "terminal"
    static let search = "magnifyingglass"
    static let copy = "doc.on.doc"
    static let copyPath = "folder"
    static let refresh = "arrow.clockwise"
    static let quit = "power"
    static let settings = "gearshape"
    static let general = "gearshape"
    static let about = "info.circle"
    static let aliases = "link"
    static let notifications = "bell.badge"
    static let choose = "folder.badge.gearshape"
    static let update = "arrow.down.circle"
    static let github = "chevron.left.forwardslash.chevron.right"
    static let copied = "checkmark.circle.fill"
    static let open = "arrow.up.forward.app"
    static let download = "arrow.down.circle"
    static let locked = "lock.shield"
    static let warning = "exclamationmark.triangle"
    /// A session is asking you something (permission, a question).
    static let needsYou = "hand.raised"
    /// Bring an interrupted session back.
    static let restore = "arrow.counterclockwise"
    /// Stop reminding: never offer this session for restore again.
    static let dismiss = "bell.slash"
    /// It could not come back, and why.
    static let cannotRestore = "exclamationmark.circle"
    /// The folded group of sessions that stopped long ago.
    static let older = "clock.arrow.circlepath"
    static let expand = "chevron.right"
    /// Start a new named session.
    static let newSession = "plus"
    /// Pick another folder for it.
    static let chooseFolder = "folder.badge.plus"

    static func state(_ state: SessionState) -> String {
        switch state {
        case .working: "waveform"
        case .waiting: "text.bubble"
        case .interrupted: "arrow.counterclockwise"
        case .closed: "checkmark.circle"
        }
    }
}

/// Key caps, like Raycast's: one bordered square per key, so `⌘K` is two.
struct KeyCap: View {
    let text: String
    /// Lit for a moment when the shortcut was pressed on the keyboard.
    var lit = false
    @Environment(\.colorScheme) private var scheme

    /// `⇧⌘C` → ["⇧", "⌘", "C"]; `↵` stays one key.
    static func keys(_ text: String) -> [String] {
        text.map(String.init)
    }

    var body: some View {
        HStack(spacing: 3) {
            ForEach(Array(Self.keys(text).enumerated()), id: \.offset) { _, key in
                Text(key)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(lit ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
                    .frame(minWidth: 20, minHeight: 20)
                    .background(
                        RoundedRectangle(cornerRadius: 5)
                            .fill(lit ? Theme.brand.opacity(0.22)
                                  : (scheme == .dark ? Color.white.opacity(0.06) : Color.black.opacity(0.035)))
                    )
                    .scaleEffect(lit ? 0.92 : 1)
                    .animation(.easeOut(duration: 0.1), value: lit)
                    .overlay(
                        RoundedRectangle(cornerRadius: 5)
                            .strokeBorder(Theme.hairline(scheme), lineWidth: 1)
                    )
            }
        }
    }
}
