import AppKit
import MossKit
import SwiftUI

/// Settings › Aliases, after Raycast's extensions table: a search field and
/// an Add button on top, then one card with Name / Command / Status columns.
/// Click a name or command to edit it in place (⏎ saves, esc cancels).
/// Everything goes through `cc-moss alias … --json`; the app never touches
/// config.toml or ~/.local/bin itself.
struct AliasesSettings: View {
    @Bindable var model: AppModel
    /// Screenshots pass canned data instead of asking cc-moss.
    var demo: AliasList?

    @State private var list: AliasList?
    @State private var loadError: String?
    @State private var search = ""
    @State private var editing: Edit?
    @State private var draftName = ""
    @State private var draftCommand = ""
    @State private var failure: (row: String, failure: AliasFailure)?
    /// A delete that failed, shown on its own row.
    @State private var rowErrors: [String: String] = [:]
    @State private var busy = false
    @Environment(\.colorScheme) private var scheme

    /// Which row is being edited; `adding` is the new row at the top.
    enum Edit: Equatable {
        case adding
        case row(String)
    }

    private static let addingID = "\u{0}new"
    private var client: MossClient? { model.client }

    init(model: AppModel, demo: AliasList? = nil) {
        self.model = model
        self.demo = demo
        _list = State(initialValue: demo)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            toolbar
            SettingsCard {
                header
                if editing == .adding {
                    editRow(id: Self.addingID)
                }
                content
            }
            footer
        }
        .task(id: model.cliPath) { await load() }
    }

    // MARK: Pieces

    private var toolbar: some View {
        HStack(spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: Icons.search)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(.secondary)
                TextField(L.t("Search aliases…", "搜索别名…"), text: $search)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
            }
            .padding(.horizontal, 12)
            .frame(height: 32)
            .background(RoundedRectangle(cornerRadius: 9).fill(Color.primary.opacity(scheme == .dark ? 0.08 : 0.05)))
            SoftButton(title: L.t("Add Alias", "添加别名"), icon: "plus", prominent: true) { startAdding() }
                .disabled(client == nil || editing == .adding)
        }
    }

    private var header: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Text(L.t("Name", "名字")).frame(width: Self.nameWidth, alignment: .leading)
                Text(L.t("Command", "命令")).frame(maxWidth: .infinity, alignment: .leading)
                Text(L.t("Status", "状态")).frame(width: Self.statusWidth, alignment: .leading)
                Spacer().frame(width: Self.actionWidth)
            }
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .frame(height: 30)
            Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
        }
    }

    @ViewBuilder private var content: some View {
        if let loadError {
            message(icon: "exclamationmark.triangle", loadError)
        } else if let list {
            let rows = list.filtered(search)
            if rows.isEmpty {
                message(icon: list.aliases.isEmpty ? "link" : Icons.search,
                        list.aliases.isEmpty
                            ? L.t("No aliases yet. Add one, and it works in every shell right away.",
                                  "还没有别名。加一个，任何 shell 里马上就能用。")
                            : L.t("No alias matches “\(search)”.", "没有和“\(search)”匹配的别名。"))
            }
            ForEach(Array(rows.enumerated()), id: \.element.id) { i, alias in
                if editing == .row(alias.name) {
                    editRow(id: alias.name)
                } else {
                    AliasRow(alias: alias, divider: i < rows.count - 1, error: rowErrors[alias.name],
                             onEdit: { startEditing(alias) },
                             onDelete: { Task { await remove(alias) } })
                }
            }
        } else {
            HStack { Spacer(); ProgressView().controlSize(.small); Spacer() }.frame(height: 46)
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let list {
                Text(L.t("Each alias is a link in \(Grouping.tilde(list.dir, home: NSHomeDirectory())) to \(Grouping.tilde(list.entry, home: NSHomeDirectory())); it runs the cc-moss command next to it, with whatever you type after it.",
                         "每个别名都是 \(Grouping.tilde(list.dir, home: NSHomeDirectory())) 里指向 \(Grouping.tilde(list.entry, home: NSHomeDirectory())) 的链接，运行右边那条 cc-moss 命令，你在后面敲的参数会接上去。"))
                if !list.dirOnPath {
                    Label(L.t("\(Grouping.tilde(list.dir, home: NSHomeDirectory())) is not on cc-moss's PATH; run `cc-moss doctor` in a terminal to check your shell.",
                              "\(Grouping.tilde(list.dir, home: NSHomeDirectory())) 不在 PATH 里；在终端里跑一下 `cc-moss doctor` 看看你的 shell。"),
                          systemImage: "exclamationmark.triangle")
                        .foregroundStyle(Theme.amber)
                }
            }
        }
        .font(.system(size: 12))
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
        .padding(.horizontal, 12)
    }

    private func message(icon: String, _ text: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: icon).foregroundStyle(.secondary)
            Text(text).font(.system(size: 13)).foregroundStyle(.secondary)
            Spacer()
        }
        .padding(12)
        .frame(minHeight: 46)
    }

    /// The row being added or edited: two fields, Save and Cancel.
    private func editRow(id: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 12) {
                FilledField(placeholder: L.t("name", "名字"), text: $draftName, monospaced: true) { save(id) }
                    .frame(width: Self.nameWidth)
                HStack(spacing: 6) {
                    Text("cc-moss").font(.system(size: 12, design: .monospaced)).foregroundStyle(.tertiary)
                    FilledField(placeholder: L.t("new --detach", "new --detach"), text: $draftCommand, monospaced: true) { save(id) }
                }
                .frame(maxWidth: .infinity)
                if busy {
                    ProgressView().controlSize(.small).frame(width: Self.statusWidth + Self.actionWidth)
                } else {
                    HStack(spacing: 6) {
                        SoftButton(title: L.t("Save", "保存"), icon: "checkmark", prominent: true) { save(id) }
                            .keyboardShortcut(.defaultAction)
                        IconButton(symbol: "xmark", help: L.t("Cancel", "取消")) { cancel() }
                            .keyboardShortcut(.cancelAction)
                    }
                    .frame(width: Self.statusWidth + Self.actionWidth, alignment: .trailing)
                }
            }
            if let failure, failure.row == id {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.circle.fill").foregroundStyle(Theme.red)
                    Text(failure.failure.message)
                        .font(.system(size: 12))
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 8)
                    if failure.failure.canReplace {
                        SoftButton(title: L.t("Replace", "替换"), icon: "arrow.triangle.2.circlepath") {
                            save(id, replace: true)
                        }
                    }
                    if failure.failure.canForce {
                        SoftButton(title: L.t("Use It Anyway", "坚持用这个名字"), icon: "exclamationmark.arrow.triangle.2.circlepath") {
                            save(id, force: true)
                        }
                        .help(L.t("Your alias will run instead of that command whenever ~/.local/bin comes first on PATH.",
                                  "只要 ~/.local/bin 在 PATH 里排得更靠前，敲这个名字就会运行你的别名而不是那个命令。"))
                    }
                }
            }
        }
        .padding(12)
        .background(Theme.brand.opacity(0.05))
    }

    // MARK: Actions

    private func load() async {
        if demo != nil { return }
        guard let client else {
            loadError = L.t("cc-moss was not found; set its location under General.", "没找到 cc-moss；在「通用」里设置它的位置。")
            return
        }
        do {
            list = try await client.aliases()
            loadError = nil
        } catch MossClient.Failure.command(let message) {
            loadError = L.t("This cc-moss has no aliases yet (\(message)). Update it to manage aliases here.",
                            "这个版本的 cc-moss 还不支持别名（\(message)）。升级之后就能在这里管理。")
        } catch {
            loadError = L.t("Couldn't read the aliases: \(error)", "读不到别名：\(error)")
        }
    }

    private func startAdding() {
        failure = nil
        draftName = ""
        draftCommand = ""
        withAnimation(.easeOut(duration: 0.15)) { editing = .adding }
    }

    private func startEditing(_ alias: Alias) {
        failure = nil
        draftName = alias.name
        draftCommand = alias.command
        editing = .row(alias.name)
    }

    private func cancel() {
        failure = nil
        withAnimation(.easeOut(duration: 0.15)) { editing = nil }
    }

    private func save(_ id: String, force: Bool = false, replace: Bool = false) {
        guard let client, !busy else { return }
        let name = draftName.trimmingCharacters(in: .whitespaces)
        let command = draftCommand.trimmingCharacters(in: .whitespaces)
        // cc-moss would just change the other alias; ask before that happens.
        if !replace, list?.replaces(name, editing: id == Self.addingID ? nil : id) == true {
            failure = (id, AliasFailure.taken(name))
            return
        }
        Task {
            busy = true
            defer { busy = false }
            do {
                if id == Self.addingID {
                    try await client.addAlias(name: name, command: command, force: force)
                } else {
                    try await client.renameAlias(from: id, to: name, command: command, force: force)
                }
                failure = nil
                editing = nil
                await load()
            } catch let f as AliasFailure {
                failure = (id, f)
            } catch {
                failure = (id, AliasFailure(code: "error", message: "\(error)"))
            }
        }
    }

    private func remove(_ alias: Alias) async {
        guard let client else { return }
        do {
            try await client.removeAlias(name: alias.name)
        } catch let f as AliasFailure {
            rowErrors[alias.name] = f.message
            return
        } catch {
            rowErrors[alias.name] = "\(error)"
            return
        }
        rowErrors[alias.name] = nil
        await load()
    }

    static let nameWidth: CGFloat = 120
    static let statusWidth: CGFloat = 110
    static let actionWidth: CGFloat = 28
}

/// One alias in the table. Hover shows the delete button; clicking the name
/// or command edits the row.
struct AliasRow: View {
    let alias: Alias
    let divider: Bool
    var error: String?
    let onEdit: () -> Void
    let onDelete: () -> Void
    @State private var hovering = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Button(action: onEdit) {
                    HStack(spacing: 12) {
                        HStack(spacing: 8) {
                            IconTile(symbol: Icons.aliases, colors: AliasRow.tint, size: 20)
                            Text(alias.name)
                                .font(.system(size: 13, weight: .medium, design: .monospaced))
                                .lineLimit(1)
                        }
                        .frame(width: AliasesSettings.nameWidth, alignment: .leading)
                        Text("cc-moss \(alias.command)")
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .help(L.t("Click to edit", "点一下就能改"))
                StatusLabel(alias: alias)
                    .frame(width: AliasesSettings.statusWidth, alignment: .leading)
                IconButton(symbol: "trash", help: L.t("Remove \(alias.name)", "删除 \(alias.name)"), action: onDelete)
                    .opacity(hovering ? 1 : 0)
                    .frame(width: AliasesSettings.actionWidth)
            }
            .padding(.horizontal, 12)
            .frame(minHeight: 46)
            .background(Color.primary.opacity(hovering ? 0.03 : 0))
            if let error {
                Label(error, systemImage: "exclamationmark.circle.fill")
                    .font(.system(size: 12))
                    .foregroundStyle(Theme.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 10)
            }
            if divider {
                Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
            }
        }
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }

    static let tint = [Color(red: 0.55, green: 0.45, blue: 0.95), Color(red: 0.42, green: 0.32, blue: 0.85)]
}

/// The status column: an icon and a word; the full reason on hover.
struct StatusLabel: View {
    let alias: Alias

    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: symbol).foregroundStyle(color)
            Text(word).foregroundStyle(alias.status.isFine ? .secondary : .primary)
        }
        .font(.system(size: 12))
        .help(help)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(help)
    }

    private var word: String {
        switch alias.status {
        case .ok: L.t("Ready", "可用")
        case .shadowed: L.t("Hidden", "被遮住")
        case .missing: L.t("No link", "没链接")
        case .broken: L.t("Broken", "失效")
        case .stale: L.t("Outdated", "过期")
        case .foreign: L.t("Name taken", "名字被占")
        case .invalid: L.t("Invalid", "无效")
        case .unknown: L.t("Unknown", "未知")
        }
    }

    private var symbol: String {
        switch alias.status {
        case .ok: "checkmark.circle.fill"
        case .shadowed, .missing, .stale: "exclamationmark.triangle.fill"
        case .broken, .foreign, .invalid: "xmark.octagon.fill"
        case .unknown: "questionmark.circle"
        }
    }

    private var color: Color {
        switch alias.status {
        case .ok: .green
        case .shadowed, .missing, .stale: Theme.amber
        case .broken, .foreign, .invalid: Theme.red
        case .unknown: .secondary
        }
    }

    private var help: String {
        guard let detail = alias.detail else { return L.t("Works in every shell.", "任何 shell 里都能用。") }
        if let fix = alias.fix { return "\(detail). \(L.t("Fix", "修复")): \(fix)" }
        return detail
    }
}

/// A small icon-only button with hover and press feedback.
struct IconButton: View {
    let symbol: String
    let help: String
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(hovering ? .primary : .secondary)
                .frame(width: 26, height: 26)
                .background(RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(hovering ? 0.08 : 0)))
                .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .help(help)
        .accessibilityLabel(help)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }
}
