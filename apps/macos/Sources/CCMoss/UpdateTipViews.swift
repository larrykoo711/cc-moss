import AppKit
import MossKit
import SwiftUI

// MARK: - Update tips (F38)

/// The top of the Sessions page: the card after an upgrade, or the bar
/// about a newer version. Light, like the provider banner, never louder.
struct UpdateTipView: View {
    @Bindable var model: AppModel
    let tip: PanelTip

    var body: some View {
        switch tip {
        case .none: EmptyView()
        case let .whatsNew(entry): WhatsNewCard(entry: entry) { withAnimation(.easeOut(duration: 0.15)) { model.whatsNew = nil } }
        case let .update(version): UpdateBar(model: model, version: version)
        }
    }
}

/// "cc-moss 0.3.2 is ready to install · See what's new", Update, ×.
struct UpdateBar: View {
    @Bindable var model: AppModel
    let version: String
    @Environment(\.colorScheme) private var scheme

    private let tint = Color.blue

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: 6).fill(tint.opacity(scheme == .dark ? 0.22 : 0.14))
                Image(systemName: Icons.update)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(tint)
            }
            .frame(width: 24, height: 24)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(UpdateTips.barText(version))
                    .font(.system(size: 12.5, weight: .medium))
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                TipLink(title: UpdateTips.seeWhatsNew, url: UpdateTips.changelogURL(version))
            }
            Spacer(minLength: 6)
            SoftButton(title: UpdateTips.update, icon: Icons.update) { model.runUpdate() }
                .focusable(false)
                .tip(L.t("Runs cc-moss update in your terminal.", "在终端里运行 cc-moss update。"))
            TipClose { withAnimation(.easeOut(duration: 0.15)) { model.dismissUpdateBar(version) } }
        }
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .padding(.vertical, 8)
        .frame(minHeight: 44)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(tint.opacity(scheme == .dark ? 0.14 : 0.08)))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(tint.opacity(0.18)))
    }
}

/// After an upgrade: what this version brings, in a few lines, once.
struct WhatsNewCard: View {
    let entry: ChangelogEntry
    let close: () -> Void
    @Environment(\.colorScheme) private var scheme

    private let tint = Theme.brand

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6).fill(tint.opacity(scheme == .dark ? 0.22 : 0.14))
                    Image(systemName: "sparkles")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(tint)
                }
                .frame(width: 24, height: 24)
                .accessibilityHidden(true)
                Text(UpdateTips.cardTitle(entry.version))
                    .font(.system(size: 11.5, weight: .medium))
                    .foregroundStyle(.secondary)
                Spacer(minLength: 6)
                TipClose(action: close)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text(entry.headline.text())
                    .font(.system(size: 13, weight: .semibold))
                    .fixedSize(horizontal: false, vertical: true)
                ForEach(Array(UpdateTips.highlights(entry).enumerated()), id: \.offset) { _, line in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Circle().fill(tint.opacity(0.7)).frame(width: 4, height: 4).offset(y: -2)
                        Text(line)
                            .font(.system(size: 12))
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .truncationMode(.tail)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            .padding(.leading, 34)
            HStack(spacing: 8) {
                TipLink(title: UpdateTips.fullChangelog, url: UpdateTips.changelogURL(entry.version))
                Spacer(minLength: 6)
                SoftButton(title: UpdateTips.gotIt, icon: "checkmark", action: close)
                    .focusable(false)
            }
            .padding(.leading, 34)
            .padding(.trailing, 4)
        }
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .padding(.top, 8)
        .padding(.bottom, 10)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(tint.opacity(scheme == .dark ? 0.12 : 0.06)))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(tint.opacity(0.18)))
    }
}

/// A quiet text link that opens the site.
private struct TipLink: View {
    let title: String
    let url: URL
    @State private var hovering = false

    var body: some View {
        Button { NSWorkspace.shared.open(url) } label: {
            HStack(spacing: 2) {
                Text(title)
                Image(systemName: "arrow.up.right").font(.system(size: 8.5, weight: .semibold))
            }
            .font(.system(size: 11.5, weight: .medium))
            .foregroundStyle(Color.accentColor.opacity(hovering ? 1 : 0.85))
            .underline(hovering)
        }
        .buttonStyle(.plain)
        .focusable(false)
        .handCursor()
        .onHover { hovering = $0 }
        .help(url.absoluteString)
    }
}

/// The × the panel's banners close with.
private struct TipClose: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: "xmark")
                .font(.system(size: 9.5, weight: .bold))
                .foregroundStyle(.tertiary)
                .frame(width: 20, height: 20)
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .accessibilityLabel(L.t("Close", "关闭"))
    }
}
