import AppKit
import MossKit
import SwiftUI

/// Settings › Aliases, for everyday people: a few things you do often, each
/// with one field for a short name to type in any terminal. Return (or the
/// tick) saves it and it works at once; hovering a row plays what typing
/// the name does. The commands behind them stay out of sight; aliases made
/// by hand live under "Custom", folded away. Everything goes through
/// `cc-moss alias … --json`; the app never touches config.toml or
/// ~/.local/bin itself.
struct AliasesSettings: View {
    @Bindable var model: AppModel
    /// Screenshots pass canned data instead of asking cc-moss.
    var demo: AliasList?

    @State private var list: AliasList?
    @State private var loadError: String?
    @State private var showCustom = false
    @State private var customLine = ""
    @State private var customError: String?

    static let tint = [Color(red: 0.55, green: 0.45, blue: 0.95), Color(red: 0.42, green: 0.32, blue: 0.85)]

    private var client: MossClient? { model.client }

    init(model: AppModel, demo: AliasList? = nil) {
        self.model = model
        self.demo = demo
        _list = State(initialValue: demo)
    }

    private var board: AliasBoard.Board { AliasBoard.board(list?.aliases ?? []) }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(L.t("Give the things you do often a short name, then type it in any terminal.",
                     "给常做的事起个短名字，在任何终端里一敲就行。"))
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
            if let loadError {
                SettingsCard {
                    CardRow(title: loadError, divider: false) { EmptyView() }
                }
            } else {
                if let list, !list.dirOnPath {
                    SettingsCard {
                        CardRow(title: L.t("Open a new terminal first", "先新开一个终端"),
                                detail: L.t("Your current terminals don't see the new names yet; a new one does. Still not? Run cc-moss install once.",
                                            "已经开着的终端还看不到新名字，新开一个就行；还不行就在终端里运行一次 cc-moss install。"),
                                divider: false) {
                            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Theme.amber)
                        }
                    }
                }
                SettingsCard(title: L.t("Everyday", "常用")) {
                    ForEach(Array(board.rows.enumerated()), id: \.element.task.id) { i, row in
                        TaskAliasRow(task: row.task, alias: row.alias, divider: i < board.rows.count - 1,
                                     save: { await save(task: row.task, field: $0, current: row.alias) },
                                     fix: { await fix(row.alias) })
                    }
                }
                custom
            }
        }
        .task(id: model.cliPath) { await load() }
    }

    // MARK: - Custom

    private var custom: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                withAnimation(.easeOut(duration: 0.18)) { showCustom.toggle() }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .semibold))
                        .rotationEffect(.degrees(showCustom ? 90 : 0))
                    Text(L.t("Custom", "自定义"))
                        .font(.system(size: 13, weight: .semibold))
                    if !board.custom.isEmpty {
                        Text("\(board.custom.count)")
                            .font(.system(size: 11, weight: .medium))
                            .padding(.horizontal, 6)
                            .background(Capsule().fill(Color.primary.opacity(0.07)))
                    }
                }
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .frame(minHeight: 30)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .handCursor()
            if showCustom {
                SettingsCard {
                    ForEach(board.custom) { alias in
                        CustomAliasRow(alias: alias) { await remove(alias) }
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        FilledField(placeholder: L.t("name = command, e.g. tl = pick --all", "名字 = 命令，比如 tl = pick --all"),
                                    text: $customLine, monospaced: true) { addCustom() }
                        Text(customError ?? L.t("For people who know cc-moss's commands. Return adds it.",
                                                "给熟悉 cc-moss 命令的人用，回车添加。"))
                            .font(.system(size: 11))
                            .foregroundStyle(customError == nil ? Theme.caption : Theme.amber)
                    }
                    .padding(12)
                }
                .transition(.opacity)
            }
        }
    }

    // MARK: - Doing

    private func load() async {
        if demo != nil { return }
        guard let client else {
            loadError = L.t("cc-moss was not found; set its location under About.", "没找到 cc-moss；在「关于」里设置它的位置。")
            return
        }
        do {
            list = try await client.aliases()
            loadError = nil
        } catch MossClient.Failure.command {
            loadError = L.t("This cc-moss is too old for aliases. Update it to use them here.",
                            "这个版本的 cc-moss 还不支持别名，升级之后就能在这里用。")
        } catch {
            loadError = L.t("Couldn't read the aliases: \(error)", "读不到别名：\(error)")
        }
    }

    /// Nil when it worked; otherwise what to show under the field.
    private func save(task: AliasBoard.Task, field: String, current: Alias?) async -> String? {
        guard let client else { return L.t("cc-moss was not found.", "没找到 cc-moss。") }
        let change = AliasBoard.change(field: field, current: current)
        // Another alias already has the name: say so instead of taking it over.
        if case let .add(name) = change, let other = list?.aliases.first(where: { $0.name == name }) {
            return L.t("“\(name)” is taken by another alias (\(other.command)).", "「\(name)」已经被另一个别名用了。")
        }
        if case let .rename(_, name) = change, list?.aliases.contains(where: { $0.name == name }) == true {
            return L.t("“\(name)” is taken by another alias.", "「\(name)」已经被另一个别名用了。")
        }
        do {
            switch change {
            case .nothing: return nil
            case let .add(name): _ = try await client.addAlias(name: name, command: task.command)
            case let .rename(old, new): _ = try await client.renameAlias(from: old, to: new, command: task.command)
            case let .remove(name): try await client.removeAlias(name: name)
            }
        } catch let f as AliasFailure {
            return f.message
        } catch {
            return "\(error)"
        }
        await load()
        return nil
    }

    /// Relink a broken or stale alias: adding it again puts the link back.
    private func fix(_ alias: Alias?) async {
        guard let client, let alias else { return }
        _ = try? await client.addAlias(name: alias.name, command: alias.command, force: true)
        await load()
    }

    private func remove(_ alias: Alias) async {
        guard let client else { return }
        try? await client.removeAlias(name: alias.name)
        await load()
    }

    private func addCustom() {
        guard let client else { return }
        guard let (name, command) = AliasBoard.parseCustom(customLine) else {
            customError = L.t("Write it as name = command.", "写成 名字 = 命令 的样子。")
            return
        }
        Task {
            do {
                _ = try await client.addAlias(name: name, command: command)
                customLine = ""
                customError = nil
                await load()
            } catch let f as AliasFailure {
                customError = f.message
            } catch {
                customError = "\(error)"
            }
        }
    }
}

/// One everyday task: what it does, a field for its short name, and on
/// hover a little terminal playing what typing the name does.
private struct TaskAliasRow: View {
    let task: AliasBoard.Task
    let alias: Alias?
    let divider: Bool
    let save: (String) async -> String?
    let fix: () async -> Void

    @State private var field: String
    @State private var problem: String?
    @State private var justSaved = false
    @State private var working = false
    @State private var hovering = false
    @State private var preview = false

    init(task: AliasBoard.Task, alias: Alias?, divider: Bool,
         save: @escaping (String) async -> String?, fix: @escaping () async -> Void) {
        self.task = task
        self.alias = alias
        self.divider = divider
        self.save = save
        self.fix = fix
        _field = State(initialValue: alias?.name ?? "")
    }

    private var dirty: Bool { AliasBoard.change(field: field, current: alias) != .nothing }
    private var broken: Bool { alias.map { !$0.status.isFine } ?? false }

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 12) {
                IconTile(symbol: task.icon, colors: AliasesSettings.tint, size: 26)
                VStack(alignment: .leading, spacing: 4) {
                    Text(task.title).font(.system(size: 14))
                    // Caption and preview share one fixed-height slot and cross-fade,
                    // so hovering never changes the row's height.
                    ZStack(alignment: .leading) {
                        caption.opacity(showsPreview ? 0 : 1)
                        if showsPreview, let name = alias?.name {
                            TerminalPreview(name: name, result: task.result).transition(.opacity)
                        }
                    }
                    .frame(height: 24, alignment: .leading)
                }
                Spacer(minLength: 12)
                trailing
                FilledField(placeholder: L.t("e.g. \(task.example)", "比如 \(task.example)"), text: $field, monospaced: true) {
                    commit()
                }
                .frame(width: 104)
            }
            .padding(12)
            .frame(minHeight: 56)
            .contentShape(Rectangle())
            .onHover { inside in hovering = inside }
            .task(id: hovering) {
                // Rest a moment before playing, so sweeping past shows nothing.
                if hovering {
                    try? await Task.sleep(for: .milliseconds(350))
                    if hovering { withAnimation(.easeOut(duration: 0.18)) { preview = true } }
                } else {
                    withAnimation(.easeOut(duration: 0.12)) { preview = false }
                }
            }
            if divider {
                Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
            }
        }
        .onChange(of: alias?.name) { _, new in if !dirty || new == field { field = new ?? "" } }
    }

    private var showsPreview: Bool { preview && alias != nil }

    @ViewBuilder private var caption: some View {
        if let problem {
            Text(problem).font(.system(size: 12)).foregroundStyle(Theme.amber).lineLimit(1)
        } else if broken, let detail = alias?.detail {
            Text(detail).font(.system(size: 12)).foregroundStyle(Theme.amber).lineLimit(1)
        } else {
            Text(task.result).font(.system(size: 12)).foregroundStyle(Theme.caption).lineLimit(1)
        }
    }

    @ViewBuilder private var trailing: some View {
        if working {
            ProgressView().controlSize(.small)
        } else if dirty {
            IconButton(symbol: "checkmark", help: L.t("Save (Return)", "保存（回车）")) { commit() }
        } else if justSaved {
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
                .transition(.scale.combined(with: .opacity))
        } else if broken {
            SoftButton(title: L.t("Fix", "修复"), icon: "wrench.adjustable") {
                Task { working = true; await fix(); working = false }
            }
        }
    }

    private func commit() {
        guard dirty, !working else { return }
        Task {
            working = true
            let error = await save(field)
            working = false
            problem = error
            guard error == nil else { return }
            withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) { justSaved = true }
            try? await Task.sleep(for: .seconds(1.6))
            withAnimation(.easeOut(duration: 0.2)) { justSaved = false }
        }
    }
}

/// A tiny terminal: the name typed out, then what happens. The box is sized
/// for the finished line from the start, so it never grows while typing.
private struct TerminalPreview: View {
    let name: String
    let result: String
    @State private var typed = 0
    @State private var done = false

    var body: some View {
        line(typed: name.count, done: true)
            .hidden()
            .overlay(alignment: .leading) { line(typed: typed, done: done) }
            .padding(.horizontal, 10)
            .frame(height: 24)
            .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Color(red: 0.11, green: 0.11, blue: 0.13)))
            .task {
                for i in 1...max(name.count, 1) {
                    try? await Task.sleep(for: .milliseconds(90))
                    typed = i
                }
                try? await Task.sleep(for: .milliseconds(220))
                withAnimation(.easeOut(duration: 0.2)) { done = true }
            }
    }

    private func line(typed: Int, done: Bool) -> some View {
        HStack(spacing: 8) {
            HStack(spacing: 0) {
                Text("$ ").foregroundStyle(Color.white.opacity(0.45))
                Text(String(name.prefix(typed))).foregroundStyle(.white)
                Rectangle().fill(Theme.brand).frame(width: 6, height: 12).opacity(done ? 0 : 1)
            }
            .font(.system(size: 12, weight: .medium, design: .monospaced))
            HStack(spacing: 8) {
                Image(systemName: "arrow.right").font(.system(size: 9, weight: .bold)).foregroundStyle(Color.white.opacity(0.4))
                Text(result).font(.system(size: 11)).foregroundStyle(Color.white.opacity(0.85)).lineLimit(1)
            }
            .opacity(done ? 1 : 0)
            .offset(x: done ? 0 : -4)
        }
    }
}

/// An alias made by hand: its name, what it runs, delete on hover.
private struct CustomAliasRow: View {
    let alias: Alias
    let remove: () async -> Void
    @State private var hovering = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text(alias.name).font(.system(size: 13, weight: .medium, design: .monospaced))
                Image(systemName: "arrow.right").font(.system(size: 9, weight: .bold)).foregroundStyle(.tertiary)
                Text(alias.command).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1)
                if !alias.status.isFine {
                    Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 11)).foregroundStyle(Theme.amber)
                        .tip(alias.detail ?? "")
                }
                Spacer()
                IconButton(symbol: "trash", help: L.t("Remove", "删除")) { Task { await remove() } }
                    .opacity(hovering ? 1 : 0)
            }
            .padding(.horizontal, 12)
            .frame(minHeight: 40)
            Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
        }
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
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
        .tip(help)
        .accessibilityLabel(help)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
    }
}
