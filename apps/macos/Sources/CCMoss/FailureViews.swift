import AppKit
import MossKit
import SwiftUI

// F25: one way to catch a failed action and hand it to Moss. Every action
// that runs cc-moss from the panel, Mo or the Moss page reports a failure
// here; the panel shows it at the bottom, whatever page is open.

extension AppModel {
    /// Something didn't work: show it until it's closed.
    func report(_ notice: FailureNotice) {
        failures.add(notice)
    }

    func report(_ action: FailureNotice.Action, _ s: Session?, error: Error) {
        report(FailureNotice(action: action, subject: s.map(FailureNotice.Subject.init), error: error))
    }

    /// Refusals among `archive|unarchive|restart --json` results.
    func report(_ action: FailureNotice.Action, refusals results: [BatchResult], among list: [Session]) {
        for r in results where r.outcome == "refused" {
            let s = list.first { $0.target == r.name || $0.title == r.name || $0.sessionId == r.sessionId }
            report(FailureNotice(action: action, batch: r, subject: s.map(FailureNotice.Subject.init)))
        }
    }

    /// Moss can take it: it's on and running.
    var mossCanFix: Bool {
        guard let status = mossDesk.status else { return false }
        return status.enabled && status.running && mossDesk.canTalk
    }

    /// "Ask Moss to fix it": the details go to Moss, and the Moss page opens.
    func askMossToFix(_ notice: FailureNotice) {
        failures.dismiss(notice.id)
        sayToMoss(notice.mossText)
        showMoss()
    }
}

/// The failure at the bottom of the panel: what didn't work, why, and what
/// to do. One at a time; "+N" when more are behind it.
struct FailureNoticeView: View {
    @Bindable var model: AppModel
    let notice: FailureNotice
    @State private var copied = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6).fill(Theme.red.opacity(scheme == .dark ? 0.22 : 0.14))
                    Image(systemName: Icons.warning)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Theme.red)
                }
                .frame(width: 24, height: 24)
                .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(notice.title)
                            .font(.system(size: 13, weight: .semibold))
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                        if model.failures.more > 0 {
                            Text("+\(model.failures.more)")
                                .font(.system(size: 10.5, weight: .semibold))
                                .monospacedDigit()
                                .foregroundStyle(.secondary)
                                .padding(.horizontal, 5)
                                .frame(height: 15)
                                .background(Capsule().fill(Color.primary.opacity(0.08)))
                                .tip(L.t("\(model.failures.more) more behind this one", "后面还有 \(model.failures.more) 条"))
                                .accessibilityLabel(L.t("\(model.failures.more) more", "还有 \(model.failures.more) 条"))
                        }
                    }
                    Text(notice.why)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 4)
                Button { withAnimation(.easeOut(duration: 0.15)) { model.failures.dismiss() } } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 9.5, weight: .bold))
                        .foregroundStyle(.tertiary)
                        .frame(width: 20, height: 20)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
                .tip(L.t("Close  esc", "关闭  esc"))
                .accessibilityLabel(L.t("Close", "关闭"))
            }
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: copied ? L.t("Copied", "已复制") : L.t("Copy Details", "复制详情"),
                           icon: copied ? "checkmark" : Icons.copy) {
                    model.copy(notice.details)
                    copied = true
                }
                .focusable(false)
                .accessibilityLabel(L.t("Copy details", "复制详情"))
                if model.mossCanFix {
                    SoftButton(title: L.t("Ask Moss to Fix It", "交给 Moss 处理"), icon: "wand.and.stars", prominent: true) {
                        model.askMossToFix(notice)
                    }
                    .focusable(false)
                    .tip(L.t("Sends Moss what happened, then opens the Moss page.", "把经过发给 Moss，再打开 Moss 页。"))
                    .accessibilityLabel(L.t("Ask Moss to fix it", "交给 Moss 处理"))
                }
            }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Theme.raised(scheme)))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Theme.red.opacity(0.3)))
        .shadow(color: .black.opacity(scheme == .dark ? 0.4 : 0.14), radius: 12, y: 4)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(notice.title). \(notice.why)")
        .onChange(of: notice.id) { _, _ in copied = false }
    }
}
