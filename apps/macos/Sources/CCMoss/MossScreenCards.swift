import AppKit
import MossKit
import SwiftUI

// /usage and /status drawn as cards instead of terminal text (T2.3
// AMEND-3). Each card sits inside the command card, which keeps a "Raw"
// toggle to the terminal text; a screen the parsers don't recognise is
// shown as text only.

enum MossScreenCard {
    /// The drawn version of a command's screen, and a badge for the card's
    /// header (the plan), or nil when there is none for this command or the
    /// screen wasn't recognised.
    static func make(command: String, text: String) -> (badge: String?, view: AnyView)? {
        switch command.lowercased() {
        case "/usage":
            guard let usage = MossUsage.parse(text) else { return nil }
            return (usage.plan, AnyView(MossUsageView(usage: usage)))
        case "/status":
            guard let status = MossStatusScreen.parse(text) else { return nil }
            return (status.plan, AnyView(MossStatusView(status: status)))
        default:
            return nil
        }
    }
}

// MARK: - Pieces both cards use

/// A section's small heading: symbol and words.
private struct SectionTitle: View {
    let symbol: String
    let title: String
    var note: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 5) {
                Text(Image(systemName: symbol))
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                    .frame(width: 13)
                Text(title)
                    .font(.system(size: 11.5, weight: .semibold))
                    .foregroundStyle(.secondary)
            }
            if let note {
                Text(note)
                    .font(.system(size: 10.5))
                    .foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.leading, 18)
            }
        }
    }
}

private struct Hairline: View {
    @Environment(\.colorScheme) private var scheme
    var body: some View { Rectangle().fill(Theme.hairline(scheme)).frame(height: 1) }
}

/// A rounded bar: a track and how much of it is used.
private struct Meter: View {
    let fraction: Double
    let color: Color
    var height: CGFloat = 6
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.primary.opacity(scheme == .dark ? 0.10 : 0.07))
                Capsule()
                    .fill(color)
                    .frame(width: fraction <= 0 ? 0 : max(height, geo.size.width * min(1, fraction)))
            }
        }
        .frame(height: height)
    }
}

// MARK: - /usage

struct MossUsageView: View {
    let usage: MossUsage
    @State private var allInsights = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if !usage.limits.isEmpty {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(Array(usage.limits.enumerated()), id: \.offset) { _, limit in limitRow(limit) }
                }
            }
            // The "Session" block is Moss's own Claude Code session, not your
            // usage: it stays in the raw text only (user 2026-10-19).
            if !usage.insights.isEmpty {
                if !usage.limits.isEmpty { Hairline() }
                insights
            }
            if !usage.tables.isEmpty {
                Hairline()
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(Array(usage.tables.enumerated()), id: \.offset) { _, table in tableView(table) }
                }
            }
            if !usage.loops.isEmpty {
                Hairline()
                loops
            }
        }
        .padding(12)
    }

    // Limits

    private func color(_ level: MossUsage.Limit.Level) -> Color {
        switch level {
        case .calm: Theme.brand.opacity(0.75)
        case .warm: Theme.amber
        case .hot: Theme.red
        }
    }

    private func limitRow(_ limit: MossUsage.Limit) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(limit.title())
                        .font(.system(size: 12.5, weight: .medium))
                        .lineLimit(1)
                    if let resets = limit.resets {
                        Text(L.t("Resets \(resets)", "\(resets) 重置"))
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                HStack(alignment: .firstTextBaseline, spacing: 1) {
                    Text("\(limit.percent)")
                        .font(.system(size: 22, weight: .semibold, design: .rounded))
                    Text("%")
                        .font(.system(size: 12, weight: .semibold, design: .rounded))
                }
                .monospacedDigit()
                .foregroundStyle(limit.level == .calm ? AnyShapeStyle(.primary) : AnyShapeStyle(color(limit.level)))
            }
            Meter(fraction: Double(limit.percent) / 100, color: color(limit.level), height: 7)
        }
        .accessibilityElement(children: .combine)
    }

    // What's using it

    private var insights: some View {
        let shown = allInsights ? usage.insights : Array(usage.insights.prefix(3))
        let more = usage.insights.count - 3
        return VStack(alignment: .leading, spacing: 9) {
            SectionTitle(symbol: "chart.pie", title: L.t("What's using it", "为什么用得多"),
                         note: usage.insightsPeriod.map { _ in
                             L.t("Last 24 hours, this Mac only. Each is separate; they don't add up.",
                                 "最近 24 小时，只算这台 Mac。各项互相独立，加起来不是 100%。")
                         })
            ForEach(Array(shown.enumerated()), id: \.offset) { _, insight in
                HStack(alignment: .top, spacing: 8) {
                    Text("\(insight.percent)%")
                        .font(.system(size: 11, weight: .bold, design: .rounded))
                        .monospacedDigit()
                        .foregroundStyle(Theme.brand)
                        .frame(minWidth: 38)
                        .frame(height: 19)
                        .background(Capsule().fill(Theme.brand.opacity(scheme == .dark ? 0.18 : 0.10)))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(insight.title())
                            .font(.system(size: 12, weight: .medium))
                            .fixedSize(horizontal: false, vertical: true)
                        if !insight.hint.isEmpty {
                            Text(insight.hint)
                                .font(.system(size: 11))
                                .foregroundStyle(.secondary)
                                .lineLimit(3)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .padding(.top, 1)
                }
            }
            if more > 0 {
                Button {
                    withAnimation(.easeOut(duration: 0.2)) { allInsights.toggle() }
                } label: {
                    HStack(spacing: 3) {
                        Text(allInsights ? L.t("Fewer", "收起") : L.t("\(more) more", "更多（\(more)）"))
                        Image(systemName: allInsights ? "chevron.up" : "chevron.down")
                            .font(.system(size: 8.5, weight: .semibold))
                    }
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(Theme.brand)
                    .padding(.leading, 46)
                    .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
            }
        }
    }

    // Who used it

    private func tableView(_ table: MossUsage.Table) -> some View {
        let top = max(1, table.rows.map(\.percent).max() ?? 1)
        let symbol = switch table.title {
        case "Skills": "sparkles"
        case "Subagents": "person.2"
        case "MCP servers": "puzzlepiece.extension"
        default: "list.bullet"
        }
        return VStack(alignment: .leading, spacing: 6) {
            SectionTitle(symbol: symbol, title: table.title())
            ForEach(Array(table.rows.enumerated()), id: \.offset) { _, row in
                HStack(spacing: 8) {
                    Text(row.name)
                        .font(.system(size: 11.5, design: row.name.hasPrefix("/") ? .monospaced : .default))
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .frame(width: 118, alignment: .leading)
                        .tip(row.name)
                    Meter(fraction: Double(row.percent) / Double(top), color: Theme.brand.opacity(0.55), height: 6)
                    Text("\(row.percent)%")
                        .font(.system(size: 11, weight: .medium))
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                        .frame(width: 34, alignment: .trailing)
                }
                .padding(.leading, 18)
            }
        }
    }

    // Loops

    private var loops: some View {
        VStack(alignment: .leading, spacing: 7) {
            SectionTitle(symbol: "arrow.triangle.2.circlepath", title: L.t("Loops", "循环任务"))
            ForEach(Array(usage.loops.enumerated()), id: \.offset) { _, loop in
                VStack(alignment: .leading, spacing: 2) {
                    Text(loop.name)
                        .font(.system(size: 12, weight: .medium))
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Text(details(loop))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                .padding(.leading, 18)
                .tip(loop.name)
            }
        }
    }

    private func details(_ loop: MossUsage.Loop) -> String {
        var parts: [String] = []
        if let every = loop.every { parts.append(every) }
        if let runs = loop.runs { parts.append(L.t(runs == "1" ? "1 run" : "\(runs) runs", "跑了 \(runs) 次")) }
        if let per = loop.perRun { parts.append(L.t("\(per) tokens a run", "每次 \(per) tokens")) }
        if let last = loop.lastRun { parts.append(L.t("last \(last)", "上次 \(last)")) }
        return parts.joined(separator: " · ")
    }
}

// MARK: - /status

struct MossStatusView: View {
    let status: MossStatusScreen
    @State private var copied = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            header
            group(symbol: "person.crop.circle", title: L.t("Account", "账号"), rows: accountRows)
            group(symbol: "bubble.left.and.text.bubble.right", title: L.t("This session", "本次会话"), rows: sessionRows)
            group(symbol: "link", title: L.t("Connections", "连接"), rows: connectionRows)
            group(symbol: "ellipsis.circle", title: L.t("Other", "其它"),
                  rows: status.others.map { Row(key: $0.key, view: AnyView(plain($0.value))) })
        }
        .padding(12)
    }

    @ViewBuilder private var header: some View {
        if status.model != nil || status.version != nil {
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 7) {
                    Text(status.model ?? "Claude Code")
                        .font(.system(size: 17, weight: .semibold))
                    if let version = status.version {
                        Text("v\(version)")
                            .font(.system(size: 10.5, weight: .medium, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 6)
                            .frame(height: 17)
                            .background(Capsule().fill(Color.primary.opacity(scheme == .dark ? 0.09 : 0.06)))
                    }
                }
                if let note = status.modelNote {
                    Text(note)
                        .font(.system(size: 11.5))
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    struct Row {
        var key: String
        var view: AnyView
    }

    @ViewBuilder private func group(symbol: String, title: String, rows: [Row]) -> some View {
        if !rows.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                SectionTitle(symbol: symbol, title: title)
                VStack(spacing: 0) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { i, row in
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Text(row.key)
                                .font(.system(size: 11.5))
                                .foregroundStyle(.secondary)
                                .frame(width: 74, alignment: .leading)
                            row.view
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 7)
                        if i < rows.count - 1 { Hairline().padding(.leading, 10) }
                    }
                }
                .background(RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .fill(Color.primary.opacity(scheme == .dark ? 0.045 : 0.03)))
            }
        }
    }

    private func plain(_ text: String, selectable: Bool = false) -> some View {
        Text(text)
            .font(.system(size: 12))
            .lineLimit(2)
            .fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled)
    }

    private var accountRows: [Row] {
        var rows: [Row] = []
        if let v = status.value("Login method") { rows.append(Row(key: L.t("Signed in", "登录方式"), view: AnyView(plain(v)))) }
        if let v = status.value("Organization") { rows.append(Row(key: L.t("Organization", "组织"), view: AnyView(plain(v)))) }
        if let v = status.value("Email") { rows.append(Row(key: L.t("Email", "邮箱"), view: AnyView(plain(v, selectable: true)))) }
        return rows
    }

    private var sessionRows: [Row] {
        var rows: [Row] = []
        if status.value("Session name") != nil || status.value("Session ID") != nil {
            let name = status.sessionName
            rows.append(Row(key: L.t("Name", "名字"), view: AnyView(
                Text(name ?? L.t("No name yet", "还没起名"))
                    .font(.system(size: 12))
                    .foregroundStyle(name == nil ? .tertiary : .primary)
            )))
        }
        if let id = status.value("Session ID") {
            rows.append(Row(key: L.t("Session ID", "会话 ID"), view: AnyView(idButton(id))))
        }
        if let cwd = status.value("cwd") {
            rows.append(Row(key: L.t("Folder", "目录"), view: AnyView(
                Text(cwd)
                    .font(.system(size: 11.5, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .tip(cwd)
            )))
        }
        if let kind = status.value("Session kind") {
            rows.append(Row(key: L.t("Kind", "类型"), view: AnyView(plain(kind))))
        }
        return rows
    }

    private func idButton(_ id: String) -> some View {
        Button {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(id, forType: .string)
            copied = true
            Task {
                try? await Task.sleep(for: .seconds(1.5))
                copied = false
            }
        } label: {
            HStack(spacing: 5) {
                Text(String(id.prefix(8)))
                    .font(.system(size: 11.5, design: .monospaced))
                    .foregroundStyle(.primary)
                Image(systemName: copied ? "checkmark" : "doc.on.doc")
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(copied ? AnyShapeStyle(Color.green) : AnyShapeStyle(.tertiary))
                if copied {
                    Text(L.t("Copied", "已复制"))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .tip(L.t("Copy \(id)", "复制 \(id)"))
    }

    private var connectionRows: [Row] {
        var rows: [Row] = []
        if let ide = status.ide {
            rows.append(Row(key: "IDE", view: AnyView(
                HStack(spacing: 6) {
                    Circle().fill(ide.connected ? Color.green : Theme.red).frame(width: 7, height: 7)
                    Text(ide.text).font(.system(size: 12)).lineLimit(2)
                }
            )))
        }
        if let mcp = status.mcp, !mcp.isEmpty {
            rows.append(Row(key: L.t("MCP", "MCP 服务"), view: AnyView(plain(mcp))))
        }
        if let cloud = status.value("Cloud sessions") {
            rows.append(Row(key: L.t("Cloud", "云端会话"), view: AnyView(plain(cloud))))
        }
        return rows
    }
}
