import AppKit
import MossKit
import SwiftUI

/// The menubar panel: search on top, a summary strip, sessions grouped by
/// project, and an action bar at the bottom (Raycast's layout, iStat Menus'
/// summary).
struct PanelView: View {
    @Bindable var model: AppModel
    @FocusState private var searchFocused: Bool
    @State var showActions = false
    @State private var toast: String?
    @Environment(\.colorScheme) private var scheme
    @Environment(\.openWindow) private var openWindow
    @Environment(\.closePanel) private var closePanel
    /// Which shortcut's key caps are lit right now ("↵" or "⌘K").
    @State private var litKey: String?

    var body: some View {
        VStack(spacing: 0) {
            searchBar
            Rectangle().fill(Theme.hairline(scheme)).frame(height: 1)
            if model.phase == .ready && !model.sessions.isEmpty {
                StateFilterBar(counts: StateCounts(model.sessions), filter: $model.stateFilter)
                    .padding(.horizontal, 10)
                    .padding(.top, 10)
                    .padding(.bottom, 2)
            }
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            actionBar
        }
        .frame(width: Theme.panelWidth, height: Theme.panelHeight)
        // Keyboard focus lives in the search field; buttons never take it,
        // so no focus ring lands on the filter or the action pill.
        .focusEffectDisabled()
        .background(Theme.surface(scheme))
        .overlay(alignment: .bottomTrailing) {
            if showActions {
                ActionsMenu(actions: actions) { showActions = false }
                    .padding(.trailing, 10)
                    .padding(.bottom, 50)
                    .transition(.opacity.combined(with: .scale(scale: 0.97, anchor: .bottomTrailing)))
            }
        }
        .overlay(alignment: .bottom) {
            if let problem = model.openProblem {
                OpenProblemCard(problem: problem, copy: copy) { model.openProblem = nil }
                    .padding(.horizontal, 10)
                    .padding(.bottom, 50)
                    .transition(.opacity.combined(with: .move(edge: .bottom)))
            }
        }
        .animation(.easeOut(duration: 0.12), value: showActions)
        .animation(.easeOut(duration: 0.15), value: model.openProblem)
        .onChange(of: model.selectedID) { _, _ in model.openProblem = nil }
        .onAppear {
            searchFocused = true
            model.terminals.rescan()
            model.refresh()
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { _ in
            searchFocused = true
            model.refresh()
        }
    }

    // MARK: Search

    private var searchBar: some View {
        HStack(spacing: 10) {
            Image(systemName: Icons.search)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(.tertiary)
            TextField(L.t("Search sessions…", "搜索会话…"), text: $model.query)
                .textFieldStyle(.plain)
                .font(.system(size: 17))
                .focused($searchFocused)
                .onSubmit { flash("↵"); perform(primaryAction) }
                .onKeyPress(.downArrow) { model.move(1); return .handled }
                .onKeyPress(.upArrow) { model.move(-1); return .handled }
                .onKeyPress(.escape) { escape(); return .handled }
                .onKeyPress(.tab, phases: .down) { press in
                    model.stateFilter = StateFilterBar.cycle(model.stateFilter, forward: !press.modifiers.contains(.shift))
                    return .handled
                }
                .onKeyPress(phases: .down) { press in shortcut(press) }
            if model.refreshing {
                ProgressView().controlSize(.small).scaleEffect(0.7)
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 50)
    }

    // MARK: Content

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .loading:
            ProgressView().controlSize(.small)
        case .notFound:
            GuideCard(
                symbol: "shippingbox",
                title: L.t("cc-moss isn't installed yet", "还没装 cc-moss"),
                message: L.t("The menubar app is a window onto the cc-moss command. Install it, then set it up:",
                             "菜单栏 App 只是 cc-moss 命令的一扇窗。先装上它，再设置一下："),
                command: "brew install larrykoo711/tap/cc-moss && cc-moss install",
                copy: copy
            )
        case .failed(let message):
            GuideCard(
                symbol: "exclamationmark.triangle",
                title: L.t("cc-moss had a problem", "cc-moss 出了点状况"),
                message: message,
                command: "cc-moss doctor",
                copy: copy
            )
        case .needsUpgrade:
            GuideCard(
                symbol: "arrow.up.circle",
                title: L.t("This app is older than your cc-moss", "这个 App 比你的 cc-moss 旧"),
                message: L.t("Update both together and they'll understand each other again:",
                             "两个一起升级，就又能对上话了："),
                command: "cc-moss update",
                copy: copy
            )
        case .ready where model.sessions.isEmpty:
            GuideCard(
                symbol: "sparkles",
                title: L.t("No sessions yet", "还没有会话"),
                message: L.t("Start Claude Code as usual and it shows up here. Or start a named one:",
                             "照常打开 Claude Code，它就会出现在这里。也可以开一个带名字的："),
                command: "cc-moss new my-first-session",
                copy: copy
            )
        case .ready where model.projects.isEmpty:
            VStack(spacing: 6) {
                Text(L.t("Nothing matches", "没有对得上的会话"))
                    .font(.system(size: 13, weight: .medium))
                Text(L.t("Try fewer words, or clear the filter.", "少打几个字，或者清掉筛选试试。"))
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }
        case .ready:
            sessionList
        }
    }

    private var sessionList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0, pinnedViews: []) {
                    ForEach(model.projects) { project in
                        SectionHeader(project: project)
                        ForEach(project.sessions) { s in
                            SessionRow(session: s, selected: s.id == model.selected?.id)
                                .id(s.id)
                                .contentShape(Rectangle())
                                .onHover { inside in if inside { model.selectedID = s.id } }
                                .onTapGesture {
                                    model.selectedID = s.id
                                    perform(primaryAction)
                                }
                        }
                    }
                }
                .padding(.horizontal, 6)
                .padding(.bottom, 6)
            }
            .scrollIndicators(.never)
            .onChange(of: model.selectedID) { _, id in
                guard let id else { return }
                withAnimation(.easeOut(duration: 0.1)) { proxy.scrollTo(id) }
            }
        }
    }

    // MARK: Action bar

    private var actionBar: some View {
        HStack(spacing: 8) {
            if let toast {
                Label(toast, systemImage: Icons.copied)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(.green)
                    .transition(.opacity)
            } else {
                LogoButton(version: model.version) { showSettings() }
            }
            Spacer()
            // Raycast's floating pill: the main action, then everything else.
            HStack(spacing: 4) {
                if let primary = primaryAction {
                    PillButton(action: { perform(primary) }) {
                        HStack(spacing: 6) {
                            ActionIcon(action: primary, symbolSize: 11)
                            Text(primary.title).font(.system(size: 12, weight: .medium))
                                .lineLimit(1).fixedSize()
                            KeyCap(text: "↵", lit: litKey == "↵")
                        }
                    }
                    .help(primary.help ?? primary.title)
                    Rectangle().fill(Theme.hairline(scheme)).frame(width: 1, height: 16)
                }
                PillButton(action: { showActions.toggle() }) {
                    HStack(spacing: 6) {
                        Text(L.t("Actions", "更多")).font(.system(size: 12))
                            .lineLimit(1).fixedSize()
                        KeyCap(text: "⌘K", lit: litKey == "⌘K" || showActions)
                    }
                }
            }
            .padding(.horizontal, 3)
            .frame(height: 32)
            .fixedSize()
            .background(Capsule().fill(Theme.raised(scheme)))
            .overlay(Capsule().strokeBorder(Theme.hairline(scheme), lineWidth: 1))
            .shadow(color: .black.opacity(scheme == .dark ? 0.35 : 0.08), radius: 6, y: 2)
        }
        .padding(.leading, 14)
        .padding(.trailing, 8)
        .frame(height: 46)
        .animation(.easeOut(duration: 0.15), value: toast)
    }

    // MARK: Actions

    private var primaryAction: PanelAction? {
        guard let s = model.selected, model.phase == .ready else { return nil }
        if s.state == .interrupted {
            // T1.3 turns this into a one-click restore.
            return PanelAction(title: L.t("Copy Restore Command", "复制找回命令"), icon: Icons.copy, shortcut: "↵",
                               help: L.t("This session stopped. Paste the command in a terminal to bring it back.",
                                         "这个会话断了。把命令贴进终端，就能把它找回来。")) {
                copy(model.restoreCommand(s))
            }
        }
        if !s.inTmux {
            // An attach command is no use for it; its folder is (user 2026-10-16).
            return PanelAction(title: L.t("Copy Folder Path", "复制目录路径"), icon: Icons.copyPath, shortcut: "↵",
                               help: L.t("Not in tmux, so it can't be opened from here. It lives in the terminal tab it was started in.",
                                         "它不在 tmux 里，没法从这里打开；它还在当初启动它的那个终端标签页里。")) {
                copy(s.cwd)
            }
        }
        let terminal = model.terminals.current
        return PanelAction(title: L.t("Open", "打开"), icon: Icons.open, shortcut: "↵",
                           image: terminal.map { TerminalPrefs.icon($0, size: 16) },
                           help: terminal.map { L.t("Open in \($0.terminal.displayName)", "在\(zhPad($0.terminal.displayName))里打开") }) {
            open(s)
        }
    }

    private var actions: [PanelAction] {
        var list: [PanelAction] = []
        // The main action leads the list too, unless it is the copy below.
        if let s = model.selected, s.inTmux || s.state == .interrupted, let primary = primaryAction {
            list.append(primary)
        }
        if let s = model.selected, model.phase == .ready {
            if s.inTmux || s.state == .interrupted {
                list.append(PanelAction(title: L.t("Copy Command", "复制命令"), icon: Icons.copy, shortcut: "⇧⌘C") {
                    copy(model.attachCommand(s))
                })
            }
            list.append(PanelAction(title: L.t("Copy Folder Path", "复制目录路径"), icon: Icons.copyPath, shortcut: s.inTmux ? "" : "↵") {
                copy(s.cwd)
            })
        }
        list.append(PanelAction(title: L.t("Refresh", "刷新"), icon: Icons.refresh, shortcut: "⌘R") { model.refresh() })
        list.append(PanelAction(title: L.t("Settings…", "设置…"), icon: Icons.settings, shortcut: "⌘,") { showSettings() })
        list.append(PanelAction(title: L.t("Quit cc-moss", "退出 cc-moss"), icon: Icons.quit, shortcut: "⌘Q") {
            NSApplication.shared.terminate(nil)
        })
        return list
    }

    private func perform(_ action: PanelAction?) {
        showActions = false
        action?.perform()
    }

    /// Open in the chosen terminal and get out of the way; if that fails,
    /// stay and say what to do instead.
    private func open(_ s: Session) {
        let panel = NSApplication.shared.keyWindow
        if model.open(s) { panel?.close() }
    }

    private func copy(_ text: String) {
        model.copy(text)
        toast = L.t("Copied", "已复制")
        Task {
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            toast = nil
        }
    }

    /// Light a shortcut's key caps briefly, so the keyboard gets feedback too.
    private func flash(_ key: String) {
        litKey = key
        Task {
            try? await Task.sleep(nanoseconds: 180_000_000)
            if litKey == key { litKey = nil }
        }
    }

    private func showSettings() {
        closePanel()
        openWindow(id: "settings")
        SettingsWindow.bringToFront()
    }

    private func escape() {
        if showActions {
            showActions = false
        } else if model.openProblem != nil {
            model.openProblem = nil
        } else if model.stateFilter != nil {
            model.stateFilter = nil
        } else if !model.query.isEmpty {
            model.query = ""
        } else {
            closePanel()
        }
    }

    private func shortcut(_ press: KeyPress) -> KeyPress.Result {
        guard press.modifiers.contains(.command) else { return .ignored }
        switch press.characters.lowercased() {
        case "k": flash("⌘K"); showActions.toggle()
        case "r": model.refresh()
        case "q": NSApplication.shared.terminate(nil)
        case ",": showSettings()
        case "c" where press.modifiers.contains(.shift):
            if let s = model.selected { copy(model.attachCommand(s)) }
        default: return .ignored
        }
        return .handled
    }
}

struct PanelAction: Identifiable {
    let id = UUID()
    let title: String
    var icon: String = Icons.copy
    let shortcut: String
    /// Shown instead of `icon`: the chosen terminal's own app icon.
    var image: NSImage?
    var help: String?
    let perform: () -> Void
}
