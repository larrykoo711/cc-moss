import AppKit
import MossKit
import SwiftUI

// MARK: - Bringing sessions back

extension AppModel {
    var restoreGroups: RestoreGroups { RestoreGroups(sessions) }

    /// Snapshot denials explain the block; the CLI still rechecks the final source context.
    func permitsLaunch(_ current: Session, action: FailureNotice.Action) -> Bool {
        guard cliCapabilities.checksWorkingDirectory else {
            report(FailureNotice(action: action, subject: .init(current), message: WorkingDirectoryPolicy.updateRequired))
            return false
        }
        if let check = current.workingDirectory, !check.ok {
            report(FailureNotice(action: action, subject: .init(current),
                                 reason: check.reason?.rawValue, message: check.explanation))
            return false
        }
        return true
    }

    /// The folded group after the search and the state filter, newest first.
    var olderShown: [Session] {
        Filter.apply(query, state: stateFilter, to: Grouping.projects(RestoreGroups(unpinned).older))
            .flatMap(\.sessions)
            .sorted { $0.lastActiveAt > $1.lastActiveAt }
    }

    /// Restores go out only by slot and full UUID; an older CLI gets no request at all.
    func permitsExactRestore(_ subject: Session?) -> Bool {
        guard cliCapabilities.restoresExactly else {
            report(FailureNotice(action: .restore, subject: subject.map(FailureNotice.Subject.init),
                                 reason: RestorePolicy.updateRequiredCode, message: RestorePolicy.updateRequired))
            return false
        }
        return true
    }

    /// The banner's button: every recently interrupted session, one exact
    /// request each. The (slot, UUID) pairs are frozen now; each is rechecked
    /// right before its turn and skipped, never retargeted, if it moved on.
    func restoreAll() {
        let recent = restoreGroups.recent
        guard !recent.isEmpty, let client, permitsExactRestore(nil) else { return }
        guard cliCapabilities.checksWorkingDirectory else {
            report(FailureNotice(action: .restore, subject: nil, reason: RestorePolicy.updateRequiredCode,
                                 message: WorkingDirectoryPolicy.updateRequired))
            return
        }
        let targets = recent.map(RestoreTarget.init)
        let scope = RestoreFlow.Scope.all(targets.map(\.slotId))
        guard restoreFlow.begin(scope) else { return }
        for s in recent { cancelRestoreSource(s.id) }
        let names = Dictionary(recent.map { ($0.id, $0.title) }, uniquingKeysWith: { a, _ in a })
        let capabilities = cliCapabilities
        Task {
            defer { refresh() }
            let outcome: RestoreReport
            do {
                outcome = try await client.restoreEach(targets, names: names, capabilities: capabilities) { target in
                    await MainActor.run {
                        target.skip(current: self.session(target.slotId), capabilities: self.cliCapabilities, name: names[target.slotId])
                    }
                }
            } catch {
                report(FailureNotice(action: .restore, subject: nil, error: error))
                restoreFlow.fail(scope, message: Self.describe(error), names: names)
                show(RestoreSummary(restored: 0, failed: targets.count))
                return
            }
            show(RestoreSummary(restored: outcome.restored.count, failed: outcome.failed.count, starting: outcome.starting.count))
            // A row keeps only its own generation's failure; a skipped or late
            // one for a renewed slot is still said out loud.
            restoreFlow.finish(scope, report: outcome.attributable(to: targets) { self.session($0) })
            offerOrReport(outcome.failed, names: recent, source: nil)
        }
    }

    /// One session, from its row, the action pill or Return.
    func restore(_ s: Session) {
        guard let current = session(s.id), current.sessionId == s.sessionId, permitsExactRestore(current),
              permitsLaunch(current, action: .restore) else { return }
        if let plan = current.restoreSource, !plan.isReady {
            if RestoreSourceFlow.canOffer(current, reason: plan.issue) {
                restoreSourceFlow.offer(current, reason: plan.issue)
                selectedID = current.id
            } else {
                report(FailureNotice(action: .restore, subject: .init(current),
                                     reason: (plan.issue ?? .resumePreflightFailed).rawValue))
                refresh()
            }
            return
        }
        restoreSourceFlow.cancel(current.id)
        run(RestoreTarget(current), names: [current])
    }

    func restoreSourceExpanded(_ session: Session) -> Bool {
        guard let choice = restoreSourceFlow[session.id] else { return false }
        return choice.ticket.matches(session) && !choice.submitting
    }

    func restoreSourceReason(_ id: String) -> RestoreReason? {
        restoreSourceFlow[id]?.issue ?? restoreFlow.failures[id]?.reason ?? session(id)?.restoreSource?.issue
    }

    func chooseRestoreDefault(_ ticket: RestoreSourceTicket) {
        _ = restoreSourceFlow.select(.defaultConfig, ticket: ticket, current: session(ticket.slotId),
                                     reason: restoreSourceReason(ticket.slotId))
    }

    func useCurrentRestoreSettings(_ ticket: RestoreSourceTicket) {
        guard let current = session(ticket.slotId), let choice = restoreSourceFlow[current.id],
              choice.ticket == ticket, choice.offersDefaultProposal(for: current) else { return }
        guard restoreSourceFlow.select(.defaultConfig, ticket: ticket, current: current, reason: restoreSourceReason(current.id)) else { return }
        confirmRestoreSource(ticket)
    }

    func showRestoreSourceOptions(_ ticket: RestoreSourceTicket) {
        restoreSourceFlow.showAdvanced(ticket, current: session(ticket.slotId), reason: restoreSourceReason(ticket.slotId))
    }

    func cancelRestoreSource(_ id: String) {
        let closingPicker = restoreSourceFlow.picker?.slotId == id
        restoreSourceFlow.cancel(id)
        if closingPicker { restoreSourcePicker?.cancel(nil) }
    }

    func chooseRestoreDirectory(_ id: String) {
        guard restoreSourcePicker == nil, let current = session(id),
              let ticket = restoreSourceFlow.beginPicker(current: current, reason: restoreSourceReason(id)) else { return }
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = false
        panel.allowsMultipleSelection = false
        panel.showsHiddenFiles = true
        panel.message = L.t("Choose the configuration folder for this restore only. Global settings won't change.",
                            "选择本次找回使用的配置目录，不修改全局设置。")
        panel.prompt = L.t("Choose Folder", "选择目录")
        panel.level = NSWindow.Level(rawValue: NSWindow.Level.statusBar.rawValue + 1)
        restoreSourcePicker = panel
        PanelHold.begin()
        panel.begin { [weak self] response in
            Task { @MainActor in
                PanelHold.end()
                guard let self else { return }
                if self.restoreSourcePicker === panel { self.restoreSourcePicker = nil }
                if response == .OK, let path = panel.url?.path {
                    _ = self.restoreSourceFlow.receiveDirectory(path, ticket: ticket, current: self.session(id),
                                                               reason: self.restoreSourceReason(id))
                } else {
                    self.restoreSourceFlow.pickerCancelled(ticket)
                }
            }
        }
    }

    func confirmRestoreSource(_ ticket: RestoreSourceTicket) {
        guard client != nil, let current = session(ticket.slotId), !restoreFlow.isRunning(current.id),
              let choice = restoreSourceFlow[current.id], choice.ticket == ticket,
              permitsExactRestore(current), permitsLaunch(current, action: .restore) else { return }
        // The CLI distinguishes missing, unreadable and damaged settings before claim/launch.
        guard let request = restoreSourceFlow.confirm(ticket, current: current, reason: restoreSourceReason(current.id)) else { return }
        run(request.ticket.target, names: [current], source: request)
    }

    private func run(_ target: RestoreTarget, names: [Session], source: RestoreSourceRequest? = nil) {
        let scope = RestoreFlow.Scope.one(target.slotId)
        // A second click while it runs does nothing.
        guard let client, permitsExactRestore(names.first), names.allSatisfy({ permitsLaunch($0, action: .restore) }),
              restoreFlow.begin(scope) else { return }
        if source == nil { for s in names { cancelRestoreSource(s.id) } }
        let capabilities = cliCapabilities
        Task {
            defer { refresh() }
            if let source, !restoreSourceFlow.isCurrent(source.ticket, session: session(source.ticket.slotId)) {
                restoreFlow.finish(scope, report: RestoreReport(results: []))
                return
            }
            do {
                // A source override is single-target; the CLI rechecks the current recorded source.
                let report: RestoreReport
                if let source { report = try await client.restore(source, capabilities: capabilities) }
                else { report = try await client.restore(target, capabilities: capabilities) }
                if let source, !restoreSourceFlow.isCurrent(source.ticket, session: session(source.ticket.slotId)) {
                    restoreFlow.finish(scope, report: RestoreReport(results: []))
                    return
                }
                if let source { restoreSourceFlow.cancel(source.ticket.slotId) }
                let current = report.attributable(to: [target]) { self.session($0) }
                restoreFlow.finish(scope, report: current)
                // No line for the target already arrives as a not_found failure: never silent (F25).
                if let notice = (current.restored.first ?? current.starting.first)?.recoveryNotice { say(notice) }
                offerOrReport(report.failed, names: names, source: source)
            } catch {
                if let source, !restoreSourceFlow.isCurrent(source.ticket, session: session(source.ticket.slotId)) {
                    restoreFlow.finish(scope, report: RestoreReport(results: []))
                    return
                }
                if let source { restoreSourceFlow.cancel(source.ticket.slotId) }
                report(.restore, names.first, error: error)
                restoreFlow.fail(scope, message: Self.describe(error),
                                 names: Dictionary(names.map { ($0.id, $0.title) }, uniquingKeysWith: { a, _ in a }))
            }
        }
    }

    /// A source problem on the same session reopens the choice; anything else is a notice.
    private func offerOrReport(_ failures: [RestoreResult], names: [Session], source: RestoreSourceRequest?) {
        for failed in failures {
            let s = names.first { $0.id == failed.id }
            if let s, let current = session(s.id), current.sessionId == s.sessionId,
               RestoreSourceFlow.canOffer(current, reason: failed.reason) {
                let failedPath: String?
                if case let .directory(path) = source?.source { failedPath = path }
                else { failedPath = nil }
                restoreSourceFlow.offer(current, reason: failed.reason, failedPath: failedPath)
            } else {
                report(FailureNotice(restore: failed, subject: s.map(FailureNotice.Subject.init)))
            }
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
                report(.close, list.count == 1 ? list.first : nil, error: error)
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
        guard s.state == .interrupted else { return nil }
        if let failure = restoreFlow.failures[s.id] {
            if let reason = failure.reason, reason != .other { return reason.explanation }
            return failure.message ?? RestoreReason.other.explanation
        }
        if let plan = s.restoreSource, plan.status == "blocked" {
            return (plan.issue ?? .configSourceUnavailable).explanation
        }
        return nil
    }

    /// Trying again will not help (nothing to resume, or no folder).
    func restoreIsHopeless(_ s: Session) -> Bool {
        restoreFlow.failures[s.id]?.reason?.isFinal == true
    }
}

struct RestoreControlPreview {
    var target: String
    var state: RestoreControlState
}

private struct RestoreControlPreviewKey: EnvironmentKey {
    static let defaultValue: RestoreControlPreview? = nil
}

extension EnvironmentValues {
    var restoreControlPreview: RestoreControlPreview? {
        get { self[RestoreControlPreviewKey.self] }
        set { self[RestoreControlPreviewKey.self] = newValue }
    }
}

/// Lightweight restore actions share feedback without the global press scale.
private struct RestoreQuietButton: View {
    let title: String
    let target: String
    var fontSize: CGFloat = 11
    let action: () -> Void
    @State private var hovering = false
    @FocusState private var focused: Bool
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.restoreControlPreview) private var preview

    private var state: RestoreControlState? { preview?.target == target ? preview?.state : nil }
    private var feedback: RestoreControlFeedback {
        RestoreControlFeedback(isEnabled: isEnabled, hovered: hovering, focused: focused, preview: state)
    }

    var body: some View {
        Button { feedback.activate(action) } label: {
            Text(title).font(.system(size: fontSize))
                .padding(.horizontal, 8).frame(minHeight: 28)
                .contentShape(RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(RestoreQuietStyle(hovered: hovering, focused: focused, preview: state))
        .disabled(state == .disabled)
        .focusable(feedback.isEnabled)
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(.return) { feedback.activate(action) ? .handled : .ignored }
        .onHover { hovering = $0 }
        .onContinuousHover { phase in
            switch phase {
            case .active: (feedback.isEnabled ? NSCursor.pointingHand : NSCursor.arrow).set()
            case .ended: NSCursor.arrow.set()
            }
        }
        .accessibilityLabel(title)
    }
}

private struct RestoreQuietStyle: ButtonStyle {
    let hovered: Bool
    let focused: Bool
    let preview: RestoreControlState?
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        let feedback = RestoreControlFeedback(isEnabled: isEnabled, hovered: hovered,
                                              pressed: configuration.isPressed, focused: focused, preview: preview)
        configuration.label
            .foregroundStyle(feedback.emphasized ? .primary : .secondary)
            .background(RoundedRectangle(cornerRadius: 6).fill(
                feedback.state == .pressed ? Color.primary.opacity(0.14)
                    : feedback.state == .hover ? Theme.selection(scheme) : .clear))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(
                feedback.showsFocus ? Color.primary.opacity(0.6) : .clear, lineWidth: 2))
            .opacity(feedback.opacity)
            .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: feedback)
    }
}

/// Original settings come first; explicit replacement stays in this same card.
struct RestoreSourceChoiceView: View {
    @Bindable var model: AppModel
    let session: Session

    var body: some View {
        if let choice = model.restoreSourceFlow[session.id], choice.ticket.matches(session), !choice.submitting {
            let proposal = choice.offersDefaultProposal(for: session)
            let enabled = choice.canConfirm || proposal
            VStack(alignment: .leading, spacing: 12) {
                Text(choice.issue == .configSourceUnknown
                     ? RestoreSettingsCopy.unknown(advanced: choice.advanced)
                     : choice.issue?.explanation ?? L.t("Check the settings location before restoring.", "找回前请检查设置位置。"))
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                if choice.advanced {
                    Picker(L.t("Settings for this restore", "本次使用的设置"), selection: mode(choice)) {
                        Text(L.t("Default", "默认设置")).tag(Optional("default"))
                        Text(L.t("Folder", "指定文件夹")).tag(Optional("directory"))
                    }
                    .pickerStyle(.segmented).labelsHidden()
                    .disabled(model.restoreSourcePicker != nil || model.restoreFlow.isRunning(session.id))
                }
                VStack(alignment: .leading, spacing: 6) {
                    if let label = RestoreSettingsCopy.locationLabel {
                        Text(label).font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                    if let path = displayPath(choice) {
                        Text(path).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary)
                            .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    } else {
                        Text(L.t("Settings location not confirmed", "设置位置未确认"))
                            .font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                    RestoreQuietButton(title: choice.advanced ? RestoreSettingsCopy.chooseFolder : RestoreSettingsCopy.changeFolder,
                                       target: choice.advanced ? "choose" : "change") {
                        if choice.advanced { model.chooseRestoreDirectory(session.id) }
                        else { model.showRestoreSourceOptions(choice.ticket) }
                    }
                    .help(RestoreSettingsCopy.folderHelp)
                    .disabled(model.restoreSourcePicker != nil || model.restoreFlow.isRunning(session.id))
                }
                HStack(spacing: 8) {
                    Spacer(minLength: 0)
                    RestoreQuietButton(title: L.t("Cancel", "取消"), target: "cancel", fontSize: 12) {
                        model.cancelRestoreSource(session.id)
                    }
                    SoftButton(title: proposal ? L.t("Use Current Settings", "使用当前设置找回") : L.t("Bring Back", "找回"),
                               icon: Icons.restore, prominent: true) {
                        if proposal { model.useCurrentRestoreSettings(choice.ticket) }
                        else { model.confirmRestoreSource(choice.ticket) }
                    }
                    .disabled(!enabled || model.restoreSourcePicker != nil || model.restoreFlow.isRunning(session.id))
                    .opacity(enabled ? 1 : 0.4)
                }
            }
            .padding(.top, 4).padding(.leading, 44).padding(.trailing, 12).padding(.bottom, 12)
        }
    }

    private func displayPath(_ choice: RestoreSourceChoice) -> String? {
        switch choice.source {
        case let .directory(path): path
        case .defaultConfig: session.restoreSource?.defaultPath
        case nil: choice.advanced ? nil : choice.issuePath ?? (choice.issue == .configSourceUnknown ? session.restoreSource?.defaultPath : session.restoreSource?.displayPath)
        }
    }

    private func mode(_ choice: RestoreSourceChoice) -> Binding<String?> {
        Binding(get: {
            switch choice.source {
            case .defaultConfig: "default"
            case .directory: "directory"
            case nil: nil
            }
        }, set: { value in
            if value == "default" { model.chooseRestoreDefault(choice.ticket) }
            if value == "directory" { model.chooseRestoreDirectory(session.id) }
        })
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
        if let s = model.restoreSummary { return s.confirmed ? .green : Theme.amber }
        return Theme.red
    }

    private var tile: some View {
        let symbol = model.restoreSummary.map { $0.confirmed ? "checkmark" : $0.starting > 0 ? "clock" : Icons.warning } ?? Icons.restore
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
                Text(s.title)
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
