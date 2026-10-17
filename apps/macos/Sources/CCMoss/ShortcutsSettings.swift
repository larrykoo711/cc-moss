import AppKit
import MossKit
import SwiftUI

/// Settings › Shortcuts: every way to get somewhere fast, in one place.
/// The panel's key from anywhere; short names to type in any terminal (each
/// set the way the panel's key is: click, type, Return); and the keys that
/// work inside the panel. The terminal names go through `cc-moss alias …
/// --json`; the app never touches config.toml or ~/.local/bin itself.
struct ShortcutsSettings: View {
    @Bindable var model: AppModel
    /// Screenshots pass canned data instead of asking cc-moss.
    var demo: AliasList?

    @State private var list: AliasList?
    @State private var loadError: String?
    @State private var showCustom = false
    @State private var customLine = ""
    @State private var customError: String?
    @State private var shortcut = PanelShortcut.load(.standard)

    private var client: MossClient? { model.client }

    init(model: AppModel, demo: AliasList? = nil) {
        self.model = model
        self.demo = demo
        _list = State(initialValue: demo)
    }

    private var board: AliasBoard.Board { AliasBoard.board(list?.aliases ?? []) }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            anywhere
            terminal
            PanelKeys()
        }
        .task(id: model.cliPath) { await load() }
    }

    // MARK: - Anywhere

    private var anywhere: some View {
        let needsTrust = shortcut.tapKey != nil && !HotKeyCenter.shared.trust.allowed
        return SettingsCard(title: L.t("Anywhere", "随时唤出")) {
            CardRow(
                title: L.t("Show the panel", "唤出面板"),
                detail: L.t("See every session from any app.", "在哪儿按一下，都能看到所有会话。"),
                divider: needsTrust
            ) {
                HotKeyRecorder(onSave: { shortcut = $0 })
            }
            if needsTrust {
                AccessibilityRow(shortcut: shortcut).transition(.opacity)
            }
        }
    }

    // MARK: - In the terminal

    private var terminal: some View {
        VStack(alignment: .leading, spacing: 6) {
            SettingsCard(title: L.t("In the terminal", "在终端里")) {
                if let loadError {
                    CardRow(title: loadError, divider: false) { EmptyView() }
                } else {
                    if let list, !list.dirOnPath {
                        CardRow(title: L.t("Open a new terminal first", "先新开一个终端"),
                                detail: L.t("Your current terminals don't see the names yet; a new one does. Still not? Run cc-moss install once.",
                                            "已经开着的终端还认不出这些名字，新开一个就行；还不行就在终端里运行一次 cc-moss install。")) {
                            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Theme.amber)
                        }
                    }
                    ForEach(Array(board.rows.enumerated()), id: \.element.task.id) { i, row in
                        TaskNameRow(task: row.task, alias: row.alias, divider: i < board.rows.count - 1,
                                    save: { await save(task: row.task, field: $0, current: row.alias) },
                                    fix: { await fix(row.alias) })
                    }
                }
            }
            Text(L.t("Type the name in any terminal and press Return.", "在任何终端里敲这个名字，回车就行。"))
                .font(.system(size: 11))
                .foregroundStyle(Theme.caption)
                .padding(.horizontal, 12)
            if loadError == nil { custom }
        }
    }

    private var custom: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                withAnimation(.easeOut(duration: 0.18)) { showCustom.toggle() }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .semibold))
                        .rotationEffect(.degrees(showCustom ? 90 : 0))
                    Text(L.t("Your own", "自己写的"))
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
            loadError = L.t("This cc-moss is too old for terminal names. Update it to use them here.",
                            "这个版本的 cc-moss 还不支持终端里的短名字，升级之后就能在这里用。")
        } catch {
            loadError = L.t("Couldn't read the names: \(error)", "读不到这些名字：\(error)")
        }
    }

    /// Nil when it worked; otherwise what to show under the field.
    private func save(task: AliasBoard.Task, field: String, current: Alias?) async -> String? {
        guard let client else { return L.t("cc-moss was not found.", "没找到 cc-moss。") }
        let change = AliasBoard.change(field: field, current: current)
        // Another name already has it: say so instead of taking it over.
        if case let .add(name) = change, list?.aliases.contains(where: { $0.name == name }) == true {
            return L.t("“\(name)” is already in use.", "「\(name)」已经被占用了。")
        }
        if case let .rename(_, name) = change, list?.aliases.contains(where: { $0.name == name }) == true {
            return L.t("“\(name)” is already in use.", "「\(name)」已经被占用了。")
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

    /// Relink a broken or stale name: adding it again puts the link back.
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

/// One everyday task: what it does, and its short name set like the
/// panel's key. Saving plays what typing the name does; so does hovering.
private struct TaskNameRow: View {
    let task: AliasBoard.Task
    let alias: Alias?
    let divider: Bool
    let save: (String) async -> String?
    let fix: () async -> Void

    @State private var problem: String?
    @State private var editing = false
    @State private var hovering = false
    @State private var preview = false
    @State private var working = false

    private var broken: Bool { alias.map { !$0.status.isFine } ?? false }

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(task.title).font(.system(size: 14))
                    // Caption and preview share one fixed-height slot and cross-fade,
                    // so the row never changes height.
                    ZStack(alignment: .leading) {
                        caption.opacity(showsPreview ? 0 : 1)
                        if showsPreview, let name = alias?.name {
                            TerminalPreview(name: name, result: task.result).transition(.opacity)
                        }
                    }
                    .frame(height: 24, alignment: .leading)
                }
                Spacer(minLength: 12)
                if broken, !editing {
                    SoftButton(title: L.t("Fix", "修复"), icon: "wrench.adjustable") {
                        Task { working = true; await fix(); working = false }
                    }
                }
                NameRecorder(name: alias?.name, example: task.example, editing: $editing, working: working) { field in
                    await commit(field)
                }
            }
            .padding(12)
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
    }

    private var showsPreview: Bool { preview && alias != nil && !editing && problem == nil }

    @ViewBuilder private var caption: some View {
        if let problem {
            Text(problem).font(.system(size: 12)).foregroundStyle(Theme.amber).lineLimit(1)
        } else if editing {
            Text(L.t("Return saves · Esc cancels · empty removes it", "回车保存 · Esc 取消 · 清空即删除"))
                .font(.system(size: 12)).foregroundStyle(Theme.caption).lineLimit(1)
        } else if broken, let detail = alias?.detail {
            Text(detail).font(.system(size: 12)).foregroundStyle(Theme.amber).lineLimit(1)
        } else {
            Text(task.result).font(.system(size: 12)).foregroundStyle(Theme.caption).lineLimit(1)
        }
    }

    /// True when it saved. Then the little terminal plays it once.
    private func commit(_ field: String) async -> Bool {
        working = true
        let error = await save(field)
        working = false
        problem = error
        guard error == nil else { return false }
        guard !field.trimmingCharacters(in: .whitespaces).isEmpty else { return true }
        withAnimation(.easeOut(duration: 0.18)) { preview = true }
        try? await Task.sleep(for: .seconds(3))
        if !hovering { withAnimation(.easeOut(duration: 0.2)) { preview = false } }
        return true
    }
}

/// A short name, recorded the way the panel's key is: a capsule showing it
/// as a keycap (or "Click to set"); click, type, Return. Esc or clicking
/// away cancels; Return on an empty field removes it.
private struct NameRecorder: View {
    let name: String?
    let example: String
    @Binding var editing: Bool
    let working: Bool
    let commit: (String) async -> Bool

    @State private var field = ""
    @State private var hover = false
    @State private var saved = false
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack {
            if editing {
                HStack(spacing: 8) {
                    TextField(L.t("e.g. \(example)", "比如 \(example)"), text: $field)
                        .textFieldStyle(.plain)
                        .font(.system(size: 13, weight: .medium, design: .monospaced))
                        .focused($focused)
                        .onSubmit(submit)
                        .onExitCommand(perform: cancel)
                        .onChange(of: focused) { _, now in if !now, !working { cancel() } }
                    if working {
                        ProgressView().controlSize(.small)
                    } else {
                        KeyCap(text: "↵")
                    }
                }
            } else {
                Button(action: start) {
                    HStack(spacing: 8) {
                        if let name {
                            Text(name)
                                .font(.system(size: 12, weight: .medium, design: .monospaced))
                                .padding(.horizontal, 7)
                                .frame(minWidth: 22, minHeight: 22)
                                .background(RoundedRectangle(cornerRadius: 5, style: .continuous).fill(Theme.raised(scheme)))
                                .overlay(RoundedRectangle(cornerRadius: 5, style: .continuous).strokeBorder(Theme.hairline(scheme)))
                                .shadow(color: .black.opacity(0.06), radius: 0.5, y: 1)
                            if saved {
                                Image(systemName: "checkmark.circle.fill")
                                    .font(.system(size: 12))
                                    .foregroundStyle(.green)
                                    .transition(.scale.combined(with: .opacity))
                            }
                        } else {
                            Text(L.t("Click to set", "点这里设置")).font(.system(size: 12)).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 6)
                        Text(name == nil ? "" : L.t("Change", "更换")).font(.system(size: 11)).foregroundStyle(.tertiary)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
                .handCursor()
            }
        }
        .padding(.horizontal, 10)
        .frame(width: 168, height: 34)
        .background(RoundedRectangle(cornerRadius: 9, style: .continuous)
            .fill(editing ? Theme.brand.opacity(0.08) : (hover ? Theme.selection(scheme) : Color.primary.opacity(0.045))))
        .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous)
            .strokeBorder(editing ? Theme.brand.opacity(0.7) : Color.primary.opacity(0.08), lineWidth: editing ? 1.5 : 1))
        .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hover = inside } }
        .animation(.spring(response: 0.3, dampingFraction: 0.8), value: editing)
    }

    private func start() {
        field = name ?? ""
        editing = true
        DispatchQueue.main.async {
            focused = true
            NSApp.sendAction(#selector(NSResponder.selectAll(_:)), to: nil, from: nil)
        }
    }

    private func cancel() {
        editing = false
    }

    private func submit() {
        let wanted = field.trimmingCharacters(in: .whitespaces)
        guard wanted != (name ?? "") else { return cancel() }
        Task {
            let ok = await commit(wanted)
            guard ok else { focused = true; return }
            editing = false
            guard !wanted.isEmpty else { return }
            withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) { saved = true }
            try? await Task.sleep(for: .seconds(1.6))
            withAnimation(.easeOut(duration: 0.2)) { saved = false }
        }
    }
}

/// The keys that work inside the panel, for reference.
private struct PanelKeys: View {
    private var keys: [(String, String)] {
        [
            ("↵", L.t("Open the selected session", "打开选中的会话")),
            ("⌘K", L.t("More actions", "更多操作")),
            ("⌘N", L.t("New session", "新会话")),
            ("⌘E", L.t("Send a message", "发消息")),
            ("⇧⌘C", L.t("Copy its command", "复制命令")),
            ("⌘R", L.t("Refresh", "刷新")),
            ("⌘,", L.t("Settings", "设置")),
        ]
    }

    var body: some View {
        SettingsCard(title: L.t("In the panel", "面板里")) {
            ForEach(Array(keys.enumerated()), id: \.offset) { i, item in
                VStack(spacing: 0) {
                    HStack {
                        Text(item.1).font(.system(size: 13))
                        Spacer()
                        KeyCap(text: item.0)
                    }
                    .padding(.horizontal, 12)
                    .frame(minHeight: 36)
                    if i < keys.count - 1 {
                        Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
                    }
                }
            }
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
