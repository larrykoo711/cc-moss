import AppKit
import MossKit
import SwiftUI

// MARK: - Archive and restart (T2.5, T2.6)

extension AppModel {
    /// Archived sessions after the state filter: none while one is set (a
    /// filter is about what's running now).
    var archivedShown: [Session] { stateFilter == nil ? archived : [] }

    /// Archived sessions that match the query.
    var archivedHits: [SearchHit] {
        guard stateFilter == nil else { return [] }
        return searchIndexes.archived(archived).search(query)
    }

    /// The provider line at the top, now.
    var providerState: ProviderBanner.State {
        if let id = providerPlanID, recoveryOperations[id] != nil || session(id)?.restartQueued == true { return .hidden }
        return providerBanner.state(sessions: sessions.filter { ProviderRecovery.state($0, confirmations: providerConfirmations) != nil })
    }

    /// Put a session that isn't running away in Archived.
    func archive(_ s: Session) {
        guard let client else { return }
        Task {
            do {
                let results = try await client.archive([s.target])
                say(Archive.summary(results))
                report(.archive, refusals: results, among: [s])
            } catch {
                report(.archive, s, error: error)
            }
            refresh()
        }
    }

    /// First click on "End and Archive": ask.
    func askToEndAndArchive(_ s: Session) {
        guard Archive.endAllowed(s) else { return }
        form = nil
        closeGate.cancel()
        endArchiveGate.request(s)
    }

    /// Second click: end it, then archive it.
    func confirmEndAndArchive() {
        guard let s = endArchiveGate.confirm(), let client else { return }
        Task {
            do {
                let results = try await client.archiveEnding([s.target])
                say(Archive.summary(results))
                report(.endAndArchive, refusals: results, among: [s])
            } catch {
                report(.endAndArchive, s, error: error)
            }
            refresh()
        }
    }

    /// Back into the list, at once.
    func unarchive(_ s: Session) {
        guard let client else { return }
        Task {
            do {
                let results = try await client.unarchive([s.target])
                say(Archive.summary(results))
                report(.unarchive, refusals: results, among: [s])
            } catch {
                report(.unarchive, s, error: error)
            }
            refresh()
        }
    }

    /// Ask Moss to delete one for good; Moss asks you first, on its page.
    func cleanUp(_ s: Session) {
        sayToMoss(Archive.cleanUpText(s.title))
        showMoss()
    }

    /// Ask Moss to delete every archived session; Moss asks you first.
    func cleanUpAll() {
        sayToMoss(Archive.cleanUpAllText)
        showMoss()
    }

    /// Every single-session recovery goes through the same tokened operation.
    func restart(_ s: Session, applyingProvider: Bool = false, cancelling: Bool = false) {
        guard client != nil, let current = session(s.id), current.sessionId == s.sessionId else { return }
        let intent: RecoveryIntent = cancelling ? .cancelQueue : applyingProvider ? .applyProvider : .restart
        guard let request = recoveryOperations.begin(current, intent: intent) else { return }
        showRecovery(current.id)
        runRecovery(request, session: current)
    }

    private func showRecovery(_ id: String) {
        selectedID = id
        providerPlanID = id
        replyOpenID = nil
    }

    private func runRecovery(_ request: RecoveryRequest, session original: Session) {
        guard let client else { return }
        Task {
            do {
                let results: [BatchResult]
                switch request.intent {
                case .restart: results = try await client.restart([request.target])
                case .applyProvider: results = try await client.applyProvider([request.target])
                case .cancelQueue: results = try await client.cancelRestart([request.target])
                case let .rebuild(plan): results = try await client.rebuildPane(plan)
                }
                recoveryOperations.complete(request, results: results, session: session(request.slotId))
            } catch {
                recoveryOperations.fail(request, notice: FailureNotice(action: .restart, subject: .init(original), error: error),
                    session: session(request.slotId))
            }
            refresh()
        }
    }

    func dismissRecovery(_ id: String) {
        recoveryOperations.dismiss(id)
        if providerPlanID == id { providerPlanID = nil }
    }

    func cancelRebuild() {
        if let id = providerPlanID { recoveryOperations.cancelConfirmation(id) }
        providerPlanID = nil
    }

    func confirmRebuild(_ id: String) {
        guard client != nil, let current = session(id) else {
            recoveryOperations.cancelConfirmation(id)
            return
        }
        guard let request = recoveryOperations.confirm(current) else { return }
        showRecovery(id)
        runRecovery(request, session: current)
    }

    /// Batch transport still gives each target exactly one operation state.
    func restartStale() {
        let targets = Restart.restartable(sessions)
        guard let client, !restartingAll, ProviderRecovery.canApplyAll(sessions, confirmations: providerConfirmations),
              targets.allSatisfy({ recoveryOperations[$0.id]?.isSubmitting != true }) else { return }
        var requests: [(Session, RecoveryRequest)] = []
        for target in targets {
            if let request = recoveryOperations.begin(target, intent: .applyProvider) { requests.append((target, request)) }
        }
        guard !requests.isEmpty else { return }
        restartingAll = true
        if let first = requests.first { showRecovery(first.0.id) }
        Task {
            do {
                let results = try await client.restartStale()
                for (target, request) in requests {
                    let matching = results.filter {
                        $0.sessionId == target.sessionId
                            || ($0.sessionId == nil && ($0.name == target.target || $0.name == target.title))
                    }
                    recoveryOperations.complete(request, results: matching, session: session(target.id))
                }
            } catch {
                for (target, request) in requests {
                    recoveryOperations.fail(request, notice: FailureNotice(action: .restart, subject: .init(target), error: error),
                        session: session(request.slotId))
                }
            }
            restartingAll = false
            refresh()
        }
    }

}

/// "End “name” and put it in Archived?" with End and Archive / Cancel.
struct EndArchiveConfirmCard: View {
    @Bindable var model: AppModel
    let session: Session

    var body: some View {
        FormCard(title: Archive.endQuestion(session.title), icon: Icons.archive) {
            FormHint(text: Archive.endHint)
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.endArchiveGate.cancel() }
                SoftButton(title: L.t("End and Archive", "结束并归档"), icon: Icons.archive, prominent: true, shortcut: "↵") {
                    model.confirmEndAndArchive()
                }
            }
        }
    }
}

// MARK: - Archived group

/// "Archived · 12" at the bottom of the list, folded by default. Open, each
/// row can be brought back or cleaned up; "Clean Up All" asks Moss.
struct ArchivedGroupHeader: View {
    @Bindable var model: AppModel
    let count: Int
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 6) {
            Button {
                withAnimation(.easeOut(duration: 0.15)) { model.archivedExpanded.toggle() }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: Icons.expand)
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(.tertiary)
                        .rotationEffect(.degrees(model.archivedExpanded ? 90 : 0))
                    Image(systemName: Icons.archive)
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(.tertiary)
                    Text(L.t("Archived", "已归档"))
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(hovering ? .primary : .secondary)
                    CountChip(count: count)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(PressStyle())
            .focusable(false)
            .onHover { hovering = $0 }
            .tip(L.t("Sessions put away. Bring one back any time.", "收起来的会话，随时可以拿回来。"))
            .accessibilityLabel(L.t("Archived, \(count)", "已归档 \(count) 个"))
            Spacer()
            if model.archivedExpanded {
                QuietButton(title: L.t("Clean Up All", "全部清理"), icon: Icons.cleanUp) { model.cleanUpAll() }
                    .tip(L.t("Moss lists what goes and asks you first. Chats go to the Trash.",
                             "Moss 会列出要删的、先问你；对话记录进废纸篓。"))
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 14)
        .padding(.bottom, 4)
    }
}

/// While searching: above the archived sessions that match.
struct ArchivedResultsHeader: View {
    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: Icons.archive)
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(.tertiary)
            Text(L.t("Archived", "已归档"))
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .padding(.top, 12)
        .padding(.bottom, 4)
    }
}

/// The small rounded count after a group's name.
struct CountChip: View {
    let count: Int

    var body: some View {
        Text("\(count)")
            .font(.system(size: 11, weight: .semibold, design: .rounded))
            .monospacedDigit()
            .foregroundStyle(.secondary)
            .padding(.horizontal, 6)
            .frame(height: 16)
            .background(Capsule().fill(Color.primary.opacity(0.07)))
    }
}

// MARK: - Provider banner

/// After a provider switch: changed settings, pending application, or queued work. Closes with ×,
/// and goes by itself 10 minutes after the switch.
struct ProviderBannerView: View {
    @Bindable var model: AppModel
    let state: ProviderBanner.State
    @Environment(\.colorScheme) private var scheme

    private var restart: Bool { if case .needsRestart = state { true } else { false } }
    /// "Restart All" only while something can be restarted from here.
    private var restartable: Bool {
        guard case let .needsRestart(n, _) = state, n > 0 else { return false }
        return ProviderRecovery.canApplyAll(model.sessions, confirmations: model.providerConfirmations)
    }
    private var tint: Color { restart ? Theme.amber : .blue }

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: 6).fill(tint.opacity(scheme == .dark ? 0.22 : 0.14))
                Image(systemName: restart ? Icons.providerRestart : Icons.provider)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(tint)
            }
            .frame(width: 24, height: 24)
            .accessibilityHidden(true)
            Text(ProviderBanner.text(state) ?? "")
                .font(.system(size: 12.5, weight: .medium))
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 6)
            if restartable {
                if model.restartingAll {
                    Text(L.t("Working…", "处理中…")).font(.system(size: 12))
                } else {
                    SoftButton(title: L.t("Apply and Restart All", "全部应用并重开"), icon: Icons.restart, prominent: true) {
                        model.restartStale()
                    }
                    .tip(L.t("Each one restarts where it is and carries on. Busy ones wait for their turn to end.",
                              "每个都在原地重开、接着聊；在忙的等这一轮结束。"))
                }
            }
            Button { withAnimation(.easeOut(duration: 0.15)) { model.providerBanner.dismiss() } } label: {
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
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .padding(.vertical, 8)
        .frame(minHeight: 44)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(tint.opacity(scheme == .dark ? 0.14 : 0.08)))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(tint.opacity(0.18)))
    }
}
