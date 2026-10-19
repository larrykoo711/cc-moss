import AppKit
import MossKit
import SwiftUI

/// What bring-all-back came to, shown on the banner for a moment.
struct RestoreSummary: Equatable {
    var restored: Int
    var failed: Int
}

// MARK: - Bringing sessions back

extension AppModel {
    var restoreGroups: RestoreGroups { RestoreGroups(sessions) }

    /// The folded group after the search and the state filter, newest first.
    var olderShown: [Session] {
        Filter.apply(query, state: stateFilter, to: Grouping.projects(RestoreGroups(unpinned).older))
            .flatMap(\.sessions)
            .sorted { $0.lastActiveAt > $1.lastActiveAt }
    }

    /// The banner's button: every recently interrupted session.
    func restoreAll() {
        let recent = restoreGroups.recent
        guard !recent.isEmpty else { return }
        run(.all(recent.map(\.id)), targets: recent.map(\.target), names: recent)
    }

    /// One session, from its row, the action pill or Return.
    func restore(_ s: Session) {
        run(.one(s.id), targets: [s.target], names: [s])
    }

    private func run(_ scope: RestoreFlow.Scope, targets: [String], names: [Session]) {
        // A second click while it runs does nothing.
        guard let client, restoreFlow.begin(scope) else { return }
        Task {
            do {
                let report = try await client.restore(targets)
                restoreFlow.finish(scope, report: report)
                switch scope {
                case .all:
                    show(RestoreSummary(restored: report.restored.count, failed: report.failed.count))
                case .one:
                    if let back = report.restored.first {
                        say(L.t("\(back.name) is back. Press ↵ to open it.", "\(back.name) 找回来了，按 ↵ 打开。"))
                    }
                }
            } catch {
                restoreFlow.fail(scope, message: Self.describe(error),
                                 names: Dictionary(names.map { ($0.id, $0.title) }, uniquingKeysWith: { a, _ in a }))
                if case .all = scope {
                    show(RestoreSummary(restored: 0, failed: names.count))
                }
            }
            refresh()
        }
    }

    /// "Stop reminding" for `list`: `cc-moss close`. The panel asks first
    /// where more than one session or anything restorable is at stake.
    func close(_ list: [Session]) {
        guard let client, !list.isEmpty else { return }
        Task {
            do {
                try await client.close(list.map(\.target))
                say(L.t("Tucked away. `cc-moss ls --all` still shows them.",
                        "已收起，`cc-moss ls --all` 还能看到。"))
            } catch {
                say(Self.describe(error))
            }
            refresh()
        }
    }

    private func show(_ summary: RestoreSummary) {
        restoreSummary = summary
        Task {
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            if restoreSummary == summary { restoreSummary = nil }
        }
    }

    func say(_ text: String) {
        notice = text
        Task {
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            if notice == text { notice = nil }
        }
    }

    /// cc-moss's own words, or what went wrong talking to it.
    static func describe(_ error: Error) -> String {
        switch error {
        case MossClient.Failure.command(let message): message
        case MossClient.Failure.timedOut: L.t("cc-moss took too long to answer.", "cc-moss 太久没回话了。")
        case MossClient.Failure.newerSchema: L.t("This app is older than your cc-moss; update both.", "这个 App 比你的 cc-moss 旧，两个一起升级吧。")
        default: "\(error)"
        }
    }

    /// Why `s` did not come back last time, in words for its row.
    func restoreProblem(_ s: Session) -> String? {
        guard s.state == .interrupted, let failure = restoreFlow.failures[s.id] else { return nil }
        if let reason = failure.reason, reason != .other { return reason.explanation }
        return failure.message ?? RestoreReason.other.explanation
    }

    /// Trying again will not help (nothing to resume, or no folder).
    func restoreIsHopeless(_ s: Session) -> Bool {
        restoreFlow.failures[s.id]?.reason?.isFinal == true
    }
}

// MARK: - Banner

/// The line at the top of the list when sessions broke: "2 sessions stopped
/// · Bring all back". While it runs it shows progress; afterwards "Brought
/// back 2" for three seconds, then it folds away.
struct RestoreBanner: View {
    @Bindable var model: AppModel
    @Environment(\.colorScheme) private var scheme

    /// Shown at all: something to offer, in progress, or just finished.
    static func shows(_ model: AppModel) -> Bool {
        model.restoreSummary != nil || model.restoreFlow.allInFlight || !model.restoreGroups.recent.isEmpty
    }

    var body: some View {
        HStack(spacing: 10) {
            tile
            text
            Spacer(minLength: 8)
            trailing
        }
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .frame(height: 44)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(tint.opacity(scheme == .dark ? 0.14 : 0.08)))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(tint.opacity(0.18)))
        .animation(.easeOut(duration: 0.15), value: model.restoreSummary)
        .animation(.easeOut(duration: 0.15), value: model.restoreFlow.allInFlight)
    }

    private var count: Int { model.restoreGroups.recent.count }

    private var tint: Color {
        if let s = model.restoreSummary { return s.failed == 0 ? .green : Theme.amber }
        return Theme.red
    }

    private var tile: some View {
        let symbol = model.restoreSummary.map { $0.failed == 0 ? "checkmark" : Icons.warning } ?? Icons.restore
        return ZStack {
            RoundedRectangle(cornerRadius: 6).fill(tint.opacity(scheme == .dark ? 0.22 : 0.14))
            Image(systemName: symbol)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(tint)
        }
        .frame(width: 24, height: 24)
        .accessibilityHidden(true)
    }

    @ViewBuilder private var text: some View {
        if let s = model.restoreSummary {
            VStack(alignment: .leading, spacing: 1) {
                Text(s.restored == 0 ? L.t("None came back", "一个都没找回来")
                                     : L.t("Brought back \(s.restored)", "找回了 \(s.restored) 个"))
                    .font(.system(size: 13, weight: .medium))
                if s.failed > 0 {
                    Text(L.t("\(s.failed) couldn't; the rows say why", "\(s.failed) 个没找回，原因写在各自那一行"))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
            }
        } else if model.restoreFlow.allInFlight {
            Text(L.t("Bringing them back…", "正在找回…"))
                .font(.system(size: 13, weight: .medium))
        } else {
            Text(count == 1 ? L.t("1 session stopped", "有 1 个会话断了")
                            : L.t("\(count) sessions stopped", "有 \(count) 个会话断了"))
                .font(.system(size: 13, weight: .medium))
        }
    }

    @ViewBuilder private var trailing: some View {
        if model.restoreSummary != nil {
            EmptyView()
        } else if model.restoreFlow.allInFlight {
            ProgressView().controlSize(.small).scaleEffect(0.7).padding(.trailing, 8)
        } else {
            SoftButton(title: L.t("Bring All Back", "全部找回"), icon: Icons.restore, prominent: true) {
                model.restoreAll()
            }
            .tip(L.t("Restart each one in tmux, right where its conversation left off.",
                      "在 tmux 里把它们一个个重新开起来，接着上次的对话。"))
        }
    }
}

// MARK: - Earlier group

/// "Stopped earlier · 12": sessions that broke more than a week ago, folded
/// by default. Open, each row can be brought back, or all of them dismissed
/// (after a second click: it can't be undone).
struct OlderGroupHeader: View {
    @Bindable var model: AppModel
    @State private var hovering = false
    @State private var confirming = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let shown = model.olderShown
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Button {
                    withAnimation(.easeOut(duration: 0.15)) { model.olderExpanded.toggle() }
                    confirming = false
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: Icons.expand)
                            .font(.system(size: 9, weight: .bold))
                            .foregroundStyle(.tertiary)
                            .rotationEffect(.degrees(model.olderExpanded ? 90 : 0))
                        Image(systemName: Icons.older)
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(.tertiary)
                        Text(L.t("Stopped earlier", "更早断开的"))
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(hovering ? .primary : .secondary)
                        Text("\(shown.count)")
                            .font(.system(size: 11, weight: .semibold, design: .rounded))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 6)
                            .frame(height: 16)
                            .background(Capsule().fill(Color.primary.opacity(0.07)))
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
                .onHover { hovering = $0 }
                .tip(L.t("Sessions that stopped more than a week ago", "一周多以前断开的会话"))
                .accessibilityLabel(L.t("Stopped earlier, \(shown.count)", "更早断开的 \(shown.count) 个"))
                Spacer()
                if model.olderExpanded && !confirming {
                    QuietButton(title: L.t("Stop Reminding All", "都不再提醒"), icon: Icons.dismiss) {
                        withAnimation(.easeOut(duration: 0.12)) { confirming = true }
                    }
                }
            }
            if confirming {
                HStack(spacing: 8) {
                    Text(L.t("Never offer these \(shown.count) for restore again?", "这 \(shown.count) 个会话以后都不再提醒？"))
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                    Spacer(minLength: 6)
                    SoftButton(title: L.t("Cancel", "取消"), icon: "xmark") {
                        withAnimation(.easeOut(duration: 0.12)) { confirming = false }
                    }
                    SoftButton(title: L.t("Stop Reminding", "不再提醒"), icon: Icons.dismiss, prominent: true) {
                        confirming = false
                        model.close(shown)
                    }
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 9).fill(Color.primary.opacity(0.035)))
                .transition(.opacity)
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 14)
        .padding(.bottom, 4)
    }
}

/// A borderless text button for secondary actions in headers.
struct QuietButton: View {
    let title: String
    let icon: String
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            HStack(spacing: 5) {
                Image(systemName: icon).font(.system(size: 10, weight: .medium))
                Text(title).font(.system(size: 11, weight: .medium)).lineLimit(1).fixedSize()
            }
            .foregroundStyle(hovering ? .primary : .secondary)
            .padding(.horizontal, 8)
            .frame(height: 22)
            .background(Capsule().fill(hovering ? Theme.selection(scheme) : .clear))
            .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hovering = inside } }
    }
}
