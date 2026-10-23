import MossKit
import SwiftUI

extension AppModel {
    func toggleProviderPlan(_ session: Session) {
        selectedID = session.id
        providerPlanID = providerPlanID == session.id ? nil : session.id
        replyOpenID = nil
    }

    func keepProviderSelection(_ session: Session) {
        guard ProviderRecovery.canKeepSelection(session) else { return }
        providerConfirmations.confirm(session)
        if let providerDefaults { providerConfirmations.save(providerDefaults) }
        providerPlanID = nil
    }
}

/// One operation card owns the title, result, confirmation and sole loading control.
struct ProviderRecoveryView: View {
    @Bindable var model: AppModel
    let session: Session
    @FocusState private var actionFocused: Bool
    @State private var detailsExpanded = false

    private var operation: RecoveryOperation? { model.recoveryOperations[session.id] }
    private var promotesApply: Bool { ProviderRecovery.promotesApply(session, confirmations: model.providerConfirmations) }

    var body: some View {
        Group {
            if let operation { operationCard(operation) } else { providerDetails }
        }
        .font(.system(size: 12))
        .padding(.leading, 44)
        .padding(.trailing, 10)
        .padding(.bottom, 10)
        .onAppear { actionFocused = true }
        .onChange(of: operation?.token) { _, _ in detailsExpanded = false }
        .onDisappear {
            model.recoveryOperations.cancelConfirmation(session.id)
            if model.providerPlanID != session.id, operation?.stage == .starting {
                model.recoveryOperations.dismiss(session.id)
            }
        }
        .onKeyPress(.escape) {
            model.recoveryOperations.cancelConfirmation(session.id)
            model.providerPlanID = nil
            return .handled
        }
    }

    private func operationCard(_ operation: RecoveryOperation) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            heading(operation.title)
            if operation.confirmation != nil {
                Text(L.t("This ends the terminal integration and other tasks in this pane. Chat history is kept.",
                         "会结束此窗格的终端集成及其他任务，聊天历史保留。"))
                    .fixedSize(horizontal: false, vertical: true)
                if session.busy {
                    Text(L.t("Rebuild after this turn. Nothing is queued.", "本轮结束后再重建，不会自动排队。"))
                        .foregroundStyle(.secondary)
                }
            } else if let explanation = operation.explanation(currentBusy: session.busy) {
                Text(explanation).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let failure = operation.failure { failureDetails(failure) }
            HStack(spacing: 8) {
                if operation.confirmation != nil {
                    secondaryAction(L.t("Cancel", "取消"), detail: L.t("Cancel without rebuilding the pane", "取消，不重建窗格")) {
                        model.cancelRebuild()
                    }
                    .focusable().focused($actionFocused)
                } else if operation.stage == .starting {
                    secondaryAction(L.t("Dismiss", "收起"), detail: L.t("Dismiss this unconfirmed status without retrying", "收起待确认状态，不会重试")) {
                        model.dismissRecovery(session.id)
                    }
                }
                Spacer(minLength: 0)
                operationAction(operation)
            }
        }
    }

    @ViewBuilder private func operationAction(_ operation: RecoveryOperation) -> some View {
        if let label = operation.progressLabel {
            HStack(spacing: 6) {
                ProgressView().controlSize(.mini)
                Text(label).font(.system(size: 12, weight: .medium))
            }
            .padding(.horizontal, 11)
            .frame(height: 28)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(label)
        } else if operation.confirmation != nil {
            SoftButton(title: L.t("Rebuild", "重建"), icon: Icons.restart, prominent: true) {
                model.confirmRebuild(session.id)
            }
            .accessibilityLabel(L.t("End this pane’s tasks and rebuild it, keeping chat history", "结束此窗格任务并重建，保留聊天历史"))
            .disabled(!model.recoveryOperations.canConfirm(session))
            .opacity(model.recoveryOperations.canConfirm(session) ? 1 : 0.45)
            .focusable()
        } else if operation.stage == .queued {
            cancelQueue
        } else {
            openSession
        }
    }

    private func failureDetails(_ failure: FailureNotice) -> some View {
        DisclosureGroup(L.t("Details", "详情"), isExpanded: $detailsExpanded) {
            VStack(alignment: .leading, spacing: 8) {
                if let reason = failure.reason {
                    Text(reason).font(.system(size: 11, design: .monospaced))
                }
                if let message = failure.message {
                    Text(message).font(.system(size: 11))
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
                HStack(spacing: 12) {
                    secondaryAction(L.t("Copy Details", "复制详情"), detail: L.t("Copy the failure details", "复制错误详情")) {
                        model.copy(failure.details)
                    }
                    if model.mossCanFix {
                        secondaryAction(L.t("Ask Moss", "小莫检查"), detail: L.t("Ask Moss to check this failure", "让小莫检查这次操作失败")) {
                            model.askMossToFix(failure)
                        }
                    }
                }
            }
            .padding(.top, 4)
        }
        .foregroundStyle(.secondary)
    }

    private var providerDetails: some View {
        VStack(alignment: .leading, spacing: 8) {
            heading(ProviderRecovery.state(session, confirmations: model.providerConfirmations)?.title
                    ?? L.t("Model and provider", "模型与渠道"))
            if let failure = session.apiFailure {
                Text(failure.kind == "model_unavailable"
                     ? L.t("The provider rejected this model. Check the provider or choose another model with /model.",
                           "渠道拒绝了这个模型。请检查渠道，或在会话中用 /model 选择其他模型。")
                     : L.t("The last API request failed. Open the session to check the error.", "上次 API 请求失败，请打开会话查看错误。"))
                    .fixedSize(horizontal: false, vertical: true)
                if let failed = failure.model {
                    Text(L.t("Rejected model: ", "失败模型：") + failed)
                        .font(.system(size: 12, design: .monospaced))
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if let plan = session.providerModel, let target = plan.model, !target.isEmpty {
                VStack(alignment: .leading, spacing: 3) {
                    Text(L.t("Target model · ", "目标模型 · ") + plan.sourceLabel).foregroundStyle(.secondary)
                    Text(target).font(.system(size: 12, design: .monospaced))
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } else {
                Text(L.t("The target model is unknown. Check the provider settings or use /model in the session.",
                         "目标模型尚未确定，请检查渠道设置，或在会话中使用 /model。"))
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if session.restartQueued {
                if session.busy {
                    Text(L.t("Runs after this turn.", "本轮结束后执行。"))
                        .foregroundStyle(.secondary)
                }
            } else if promotesApply {
                Text(session.busy ? L.t("Restarts after this turn.", "本轮结束后重开。")
                     : L.t("Restart with this model. Keep the history.", "按此模型重开，历史保留。"))
                    .foregroundStyle(.secondary)
            }
            HStack(spacing: 8) {
                providerSecondary
                Spacer(minLength: 0)
                providerPrimary.fixedSize()
            }
        }
    }

    @ViewBuilder private var providerPrimary: some View {
        if session.restartQueued {
            cancelQueue
        } else if ProviderRecovery.canApply(session) {
            if promotesApply {
                SoftButton(title: session.busy ? L.t("Restart Later", "稍后重开") : L.t("Restart", "重开"),
                           icon: Icons.restart, prominent: true) {
                    model.restart(session, applyingProvider: true)
                }
                .accessibilityLabel(session.busy
                    ? L.t("Apply this model and restart after the current turn", "本轮结束后应用此模型并重开会话")
                    : L.t("Apply this model and restart the session", "应用此模型并重开会话"))
                .help(L.t("Restart using the target model and keep the conversation history. This does not resend your last request or verify API availability.",
                          "按目标模型重开并保留历史记录，不会自动重发上次请求，也不代表 API 已验证可用。"))
                .focusable().focused($actionFocused)
            } else {
                secondaryAction(session.busy ? L.t("Use Model After Turn", "本轮后按此模型重开") : L.t("Restart with Model", "按此模型重开"),
                                detail: L.t("Optionally restart with this model and keep the history. Busy sessions restart after this turn; API availability is not verified.",
                                            "可按此模型重开并保留历史；忙碌时本轮后执行，不代表 API 已验证可用。")) {
                    model.restart(session, applyingProvider: true)
                }
                .focusable().focused($actionFocused)
            }
        } else if !session.inTmux {
            SoftButton(title: L.t("Move into tmux", "移进 tmux"), icon: Icons.adopt, prominent: true) {
                model.providerPlanID = nil
                model.askToAdopt(session)
            }
            .focusable().focused($actionFocused)
        } else {
            openSession
        }
    }

    private var cancelQueue: some View {
        SoftButton(title: L.t("Cancel Queue", "取消排队"), icon: "xmark") { model.restart(session, cancelling: true) }
            .accessibilityLabel(L.t("Cancel the queued restart", "取消已排队的会话重开"))
            .help(L.t("Cancel this queued restart without restarting the session.", "取消本次排队，不会重开会话。"))
            .focusable().focused($actionFocused)
    }

    private var openSession: some View {
        SoftButton(title: L.t("Open Session", "打开会话"), icon: Icons.open, prominent: true) { model.open(session) }
            .focusable().focused($actionFocused)
    }

    @ViewBuilder private var providerSecondary: some View {
        if let failure = session.apiFailure, model.mossCanFix {
            secondaryAction(L.t("Ask Moss", "小莫检查"), detail: L.t("Ask Moss to check this API failure", "让小莫检查这次 API 故障")) {
                let details = [failure.status.map { "HTTP \($0)" }, failure.model].compactMap { $0 }.joined(separator: " · ")
                model.askMossToFix(FailureNotice(action: .send, subject: .init(session), reason: failure.kind, message: details))
            }
        } else if ProviderRecovery.canKeepSelection(session) {
            secondaryAction(L.t("Keep Current", "保持当前"), detail: L.t("Keep the current selection for this configuration change without restarting or changing the model", "保留当前选择，仅确认本次配置提示，不重开或改动模型")) {
                model.keepProviderSelection(session)
            }
        }
    }

    private func heading(_ title: String) -> some View {
        Button { model.toggleProviderPlan(session) } label: {
            HStack(spacing: 6) {
                Text(title).font(.system(size: 12, weight: .semibold))
                Image(systemName: "chevron.up").font(.system(size: 9, weight: .medium)).foregroundStyle(.secondary)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title + L.t(", collapse", "，收起"))
    }

    private func secondaryAction(_ title: String, detail: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).font(.system(size: 12)).foregroundStyle(.secondary)
                .lineLimit(2).multilineTextAlignment(.leading)
                .fixedSize(horizontal: false, vertical: true)
                .frame(minHeight: 28, alignment: .leading).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(detail)
        .help(detail)
    }
}
