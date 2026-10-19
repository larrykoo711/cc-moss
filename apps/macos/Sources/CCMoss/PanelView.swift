import AppKit
import MossKit
import SwiftUI

/// The menubar panel: search on top, a summary strip, sessions grouped by
/// project, and an action bar at the bottom (Raycast's layout, iStat Menus'
/// summary).
struct PanelView: View {
    @Bindable var model: AppModel
    @FocusState private var searchFocused: Bool
    @FocusState private var replyFocused: Bool
    /// The open menu: one session's, or cc-moss's own.
    @State var menu: PanelMenu?
    @State private var toast: String?
    @Environment(\.colorScheme) private var scheme
    @Environment(\.openWindow) private var openWindow
    @Environment(\.closePanel) private var closePanel
    /// The key cap lit right now ("↵"), so the keyboard gets feedback too.
    @State private var litKey: String?
    /// The row under the pointer, which shows its buttons.
    @State var hoveredID: String?
    /// The menu's items as they were when it opened. Rebuilding them while
    /// open moved items under the pointer: a click meant for "Write a Note"
    /// could land on something else.
    @State private var menuSections: [[PanelAction]] = []
    /// The lit menu item; ↑↓ move it, Return runs it.
    @State private var menuIndex: Int?
    @State private var rightClicks: Any?
    @State private var scrolls: Any?
    /// Hover waits while the list scrolls (user 2026-10-18).
    @State private var hoverGate = HoverGate()

    var body: some View {
        HStack(spacing: 0) {
            NavRail(model: model, settings: showSettings)
            page
        }
        .frame(width: Theme.panelWidth, height: model.panelHeight)
        .background(Theme.surface(scheme))
        .onChange(of: model.queueItems.isEmpty) { _, empty in if empty { model.askQueue.emptied() } }
        .onChange(of: model.page) { _, page in if page == .sessions { searchFocused = true } }
    }

    private var page: some View {
        VStack(spacing: 0) {
            if model.page == .queue {
                QueuePage(model: model)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if model.page == .moss {
                MossPage(model: model, settings: showMossSettings) { press in
                    if press.key == .escape { escape(); return .handled }
                    return shortcut(press)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                searchBar
                Rectangle().fill(Theme.hairline(scheme)).frame(height: 1)
                if AccessLostBanner.shows() {
                    AccessLostBanner()
                        .padding(.horizontal, 10)
                        .padding(.top, 10)
                        .transition(.opacity.combined(with: .move(edge: .top)))
                }
                if model.phase == .ready && !model.sessions.isEmpty {
                    StateFilterBar(counts: StateCounts(model.sessions), filter: $model.stateFilter)
                        .padding(.horizontal, 10)
                        .padding(.top, 10)
                        .padding(.bottom, 2)
                    if RestoreBanner.shows(model) {
                        RestoreBanner(model: model)
                            .padding(.horizontal, 10)
                            .padding(.top, 8)
                            .transition(.opacity.combined(with: .move(edge: .top)))
                    }
                }
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    // Room under the last row for the nudge floating over it.
                    .safeAreaPadding(.bottom, QueueNudge.shows(model) ? 56 : 0)
                    .overlay(alignment: .bottom) {
                        if QueueNudge.shows(model) {
                            QueueNudge(model: model)
                                .padding(.horizontal, 10)
                                .padding(.bottom, 8)
                                .transition(.opacity.combined(with: .move(edge: .bottom)))
                        }
                    }
                // The session list's own bar (cc-moss's menu, new session);
                // To Do and Moss end with their own controls.
                actionBar
            }
        }
        .frame(width: Theme.pageWidth, height: model.panelHeight)
        // Keyboard focus lives in the search field; buttons never take it,
        // so no focus ring lands on the filter or the action pill.
        .focusEffectDisabled()
        .tooltipLayer()
        .background(Theme.surface(scheme))
        .overlayPreferenceValue(MenuAnchorKey.self) { anchor in
            if let menu {
                // Opened without a click (screenshots start with it open): build the items now.
                let sections = menuSections.isEmpty ? sections(for: menu) : menuSections
                GeometryReader { geo in
                    ZStack(alignment: .topLeading) {
                        // A click anywhere else closes it, as a menu does.
                        Color.clear.contentShape(Rectangle()).onTapGesture { closeMenu() }
                        ActionsMenu(sections: sections, highlighted: $menuIndex) { closeMenu() }
                            .offset(menuOffset(menu, sections: sections, row: anchor.map { geo[$0] }, in: geo.size))
                    }
                }
                .transition(.opacity)
            }
        }
        .overlay {
            if model.form != nil || model.closeGate.asking {
                ZStack(alignment: .bottom) {
                    FormScrim { dismissForm() }
                    Group {
                        switch model.form {
                        case .newSession: NewSessionForm(model: model) { s in open(s) }
                        case .note(let s): NoteForm(model: model, session: s)
                        case .message(let s): MessageForm(model: model, session: s)
                        case .rename(let s): RenameForm(model: model, session: s)
                        case nil:
                            if let list = model.closeGate.pending { CloseConfirmCard(model: model, sessions: list) }
                        }
                    }
                    .padding(.bottom, 54)
                    .onKeyPress(.escape) { dismissForm(); return .handled }
                }
                .transition(.opacity)
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
        .animation(.easeOut(duration: 0.12), value: menu)
        .animation(.easeOut(duration: 0.15), value: model.form)
        .animation(.easeOut(duration: 0.15), value: model.closeGate.asking)
        // A form takes the keyboard from the search field, and gives it back when it closes.
        .onChange(of: model.form) { _, form in searchFocused = form == nil }
        .animation(.easeOut(duration: 0.15), value: model.openProblem)
        .animation(.easeOut(duration: 0.2), value: RestoreBanner.shows(model))
        .animation(.easeOut(duration: 0.2), value: AccessLostBanner.shows())
        .animation(.easeOut(duration: 0.2), value: model.askDesk)
        .animation(.easeOut(duration: 0.15), value: model.page)
        .animation(.easeOut(duration: 0.2), value: QueueNudge.shows(model))
        .onChange(of: model.selectedID) { _, id in
            model.openProblem = nil
            // Selecting another row (the keyboard, a click) closes a reply line.
            if let open = model.replyOpenID, open != id { model.closeReply() }
        }
        .onChange(of: model.replyOpenID) { _, id in
            if id != nil { replyFocused = true } else if model.form == nil { searchFocused = true }
        }
        .onAppear {
            searchFocused = true
            model.terminals.rescan()
            model.refresh()
            // Right-click a row: everything for that session, as in Finder.
            if rightClicks == nil {
                rightClicks = NSEvent.addLocalMonitorForEvents(matching: .rightMouseDown) { event in
                    guard event.window is KeyPanel, model.form == nil, let id = hoveredID else { return event }
                    model.selectedID = id
                    openMenu(.session(id, full: true))
                    return nil
                }
            }
            // Scrolling: drop the hovered row's buttons; rows passing under
            // the pointer aren't hovered until it moves again.
            if scrolls == nil {
                scrolls = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { event in
                    if event.window is KeyPanel {
                        hoverGate.scrolled(pointer: NSEvent.mouseLocation)
                        if hoveredID != nil, menu == nil { hoveredID = nil }
                    }
                    return event
                }
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { note in
            // Only the panel's own window; back from a dropdown or the folder
            // picker, an open form keeps the keyboard.
            if note.object is KeyPanel, model.form == nil { searchFocused = true }
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
                .onSubmit {
                    flash("↵")
                    // Return answers the "stop reminding?" card when it is up,
                    // and runs the lit item when a menu is.
                    if model.closeGate.asking {
                        model.confirmClose()
                    } else if menu != nil {
                        runMenuItem()
                    } else {
                        perform(primaryAction)
                    }
                }
                .onKeyPress(.downArrow) { menu != nil ? moveInMenu(1) : model.move(1); return .handled }
                .onKeyPress(.upArrow) { menu != nil ? moveInMenu(-1) : model.move(-1); return .handled }
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
        case .ready where model.sessions.isEmpty && model.moss == nil:
            GuideCard(
                symbol: "sparkles",
                title: L.t("No sessions yet", "还没有会话"),
                message: L.t("Start Claude Code as usual and it shows up here. Or start a named one:",
                             "照常打开 Claude Code，它就会出现在这里。也可以开一个带名字的："),
                command: "cc-moss new my-first-session",
                copy: copy
            )
        case .ready where model.projects.isEmpty && model.olderShown.isEmpty && model.pinnedMoss == nil && model.pinnedShown.isEmpty:
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
                    if let moss = model.pinnedMoss {
                        MossHeader()
                        row(moss)
                    }
                    let pinned = model.pinnedShown
                    if !pinned.isEmpty {
                        PinnedHeader()
                        ForEach(pinned) { s in row(s) }
                    }
                    ForEach(model.projects) { project in
                        SectionHeader(project: project)
                        ForEach(project.sessions) { s in row(s) }
                    }
                    if !model.olderShown.isEmpty {
                        OlderGroupHeader(model: model)
                        if model.olderExpanded {
                            ForEach(model.olderShown) { s in row(s) }
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
            // The row you just wrote to moved: bring it into view, once the
            // refreshed order is drawn.
            .onChange(of: model.revealTick) { _, _ in
                guard let id = model.selectedID else { return }
                DispatchQueue.main.async {
                    withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(id) }
                }
            }
        }
    }

    private func row(_ s: Session) -> some View {
        // A click only selects; opening is deliberate: the button that
        // appears at the end of the hovered row, the action pill below, or
        // Return (user 2026-10-16). The row carries its own actions: the
        // few used most, then "⋯" for the rest (user 2026-10-18).
        let menuOpen = menuRow == s.id
        let shown = s.id == hoveredID || menuOpen
        let actions = RowActions(s, hopeless: model.restoreIsHopeless(s))
        // A button, not a tap gesture: inside the scrolling list a tap
        // gesture never fired once the panel had the keyboard, so a click
        // selected nothing (user 2026-10-18). Its own buttons still win.
        // Moss's second line is its newest reply, not what was last typed into it.
        var shownSession = s
        if s.isMoss {
            shownSession.lastPrompt = MossRowLine.of(model.mossDesk.messages)
        }
        let lineOpen = model.replyOpenID == s.id
        return VStack(spacing: 0) {
            rowButton(s, shownSession: shownSession, menuOpen: menuOpen, shown: shown, actions: actions, lineOpen: lineOpen)
            if lineOpen {
                ReplyLineView(model: model, session: s, focused: $replyFocused)
                    .transition(.opacity)
            }
        }
        .background(RoundedRectangle(cornerRadius: 8).fill(lineOpen ? Theme.selection(scheme) : .clear))
        .clipped()
        .animation(.easeOut(duration: 0.18), value: lineOpen)
        .id(s.id)
    }

    private func rowButton(_ s: Session, shownSession: Session, menuOpen: Bool, shown: Bool, actions: RowActions,
                           lineOpen: Bool) -> some View {
        Button {
            model.selectedID = s.id
            // Waiting for you: a reply line opens right under it.
            model.openReply(s)
        } label: {
            SessionRow(session: shownSession, selected: s.id == model.selected?.id, hovered: shown,
                       actions: shown ? actions.inline.map { action($0, for: s) } : [],
                       more: shown && !actions.more.isEmpty ? { toggleMenu(.session(s.id, full: false)) } : nil,
                       menuOpen: menuOpen,
                       perform: { perform($0) },
                       problem: model.restoreProblem(s),
                       pinned: model.pins.contains(s.id),
                       lineOpen: lineOpen,
                       pin: s.isMoss ? nil : { model.togglePin(s) },
                       restoring: s.state == .interrupted && model.restoreFlow.isRunning(s.id))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusable(false)
        .anchorPreference(key: MenuAnchorKey.self, value: .bounds) { menuOpen ? $0 : nil }
        .onContinuousHover { phase in
            switch phase {
            case .active:
                // Rows under an open menu stay put, and rows sliding
                // under a still pointer during a scroll aren't hovered.
                // Hover only shows the row's buttons; a click or the
                // keyboard selects (user 2026-10-18: passing over rows on
                // the way to a button changed what it would open).
                guard menu == nil, hoveredID != s.id, hoverGate.allowsHover(pointer: NSEvent.mouseLocation) else { return }
                hoveredID = s.id
            case .ended:
                if hoveredID == s.id { hoveredID = nil }
            }
        }
        .handCursor()
    }

    // MARK: Action bar

    private var actionBar: some View {
        HStack(spacing: 8) {
            if let toast {
                Label(toast, systemImage: Icons.copied)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(.green)
                    .transition(.opacity)
            } else if let notice = model.notice {
                Text(notice)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .tip(notice)
                    .transition(.opacity)
            } else {
                // The version gives way first when the main action is long.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 4) {
                        LogoButton(version: model.version, open: menu == .app) { toggleMenu(.app) }
                        if model.phase == .ready { NewSessionButton { model.startNewSession() } }
                    }
                    HStack(spacing: 4) {
                        LogoButton(version: model.version, compact: true, open: menu == .app) { toggleMenu(.app) }
                        if model.phase == .ready { NewSessionButton { model.startNewSession() } }
                    }
                }
            }
            Spacer()
            // Raycast's floating pill with the main action; the rest live on
            // each session's row (user 2026-10-18).
            if model.page == .sessions, let primary = primaryAction {
                PillButton(action: { perform(primary) }) {
                    HStack(spacing: 6) {
                        ActionIcon(action: primary, symbolSize: 11)
                        Text(primary.title).font(.system(size: 12, weight: .medium))
                            .lineLimit(1).fixedSize()
                        KeyCap(text: "↵", lit: litKey == "↵")
                    }
                }
                .tip(primary.help ?? primary.title)
                .padding(.horizontal, 3)
                .frame(height: 32)
                .fixedSize()
                .background(Capsule().fill(Theme.raised(scheme)))
                .overlay(Capsule().strokeBorder(Theme.hairline(scheme), lineWidth: 1))
                .shadow(color: .black.opacity(scheme == .dark ? 0.35 : 0.08), radius: 6, y: 2)
            }
        }
        .padding(.leading, 14)
        .padding(.trailing, 8)
        .frame(height: 46)
        .animation(.easeOut(duration: 0.15), value: toast)
        .animation(.easeOut(duration: 0.15), value: model.notice)
    }

    // MARK: Actions

    private var primaryAction: PanelAction? {
        guard let s = model.selected, model.phase == .ready else { return nil }
        return action(RowActions(s, hopeless: model.restoreIsHopeless(s)).primary, for: s)
    }

    /// What one row action looks like and does.
    /// Pin to the top of the list, or unpin (⌘P).
    private func pinAction(for s: Session) -> PanelAction {
        let pinned = model.pins.contains(s.id)
        return PanelAction(title: pinned ? L.t("Unpin", "取消置顶") : L.t("Pin", "置顶"),
                           icon: pinned ? "pin.slash" : "pin", shortcut: "⌘P") {
            model.togglePin(s)
        }
    }

    private func action(_ kind: RowAction, for s: Session) -> PanelAction {
        switch kind {
        case .open:
            let terminal = model.terminals.current
            return PanelAction(title: L.t("Open", "打开"), icon: Icons.open, shortcut: "↵",
                               image: terminal.map { TerminalPrefs.icon($0, size: 16) },
                               help: terminal.map { L.t("Open in \($0.terminal.displayName)", "在\(zhPad($0.terminal.displayName))里打开") }) {
                open(s)
            }
        case .bringBack:
            return PanelAction(title: model.restoreFlow.isRunning(s.id) ? L.t("Bringing Back…", "正在找回…") : L.t("Bring Back", "找回"),
                               icon: Icons.restore, shortcut: "↵",
                               help: L.t("Restart it in tmux, right where the conversation left off.",
                                         "在 tmux 里把它重新开起来，接着上次的对话。")) {
                model.restore(s)
            }
        case .stopReminding:
            // Nothing to bring back: offer to stop reminding instead.
            return PanelAction(title: L.t("Stop Reminding", "不再提醒"), icon: Icons.dismiss, shortcut: "↵",
                               help: L.t("Never offer this one for restore again. `cc-moss ls --all` still lists it.",
                                         "以后不再提示找回它；`cc-moss ls --all` 里仍然看得到。")) {
                model.close([s])
            }
        case .copyPath:
            // Outside tmux an attach command is no use; its folder is (user 2026-10-16).
            let main = !s.inTmux && s.state != .interrupted
            return PanelAction(title: L.t("Copy Folder Path", "复制目录路径"), icon: Icons.copyPath, shortcut: main ? "↵" : "",
                               help: main ? L.t("Not in tmux, so it can't be opened from here. It lives in the terminal tab it was started in.",
                                                "它不在 tmux 里，没法从这里打开；它还在当初启动它的那个终端标签页里。") : nil) {
                copy(s.cwd)
            }
        case .copyCommand:
            return PanelAction(title: L.t("Copy Command", "复制命令"), icon: Icons.copy, shortcut: "⇧⌘C") {
                copy(model.attachCommand(s))
            }
        case .copyRestoreCommand:
            // The way back that needs no app: paste it in a terminal.
            return PanelAction(title: L.t("Copy Restore Command", "复制找回命令"), icon: Icons.copy, shortcut: "⇧⌘C") {
                copy(model.restoreCommand(s))
            }
        case .message:
            return PanelAction(title: L.t("Send a Message…", "发消息…"), icon: Icons.message, shortcut: "⌘E",
                               help: L.t("Type something into this session, as if in its terminal",
                                         "替你在这个会话里输入一段话，就像在它的终端里打字")) {
                model.writeMessage(s)
            }
        case .rename:
            return PanelAction(title: L.t("Rename…", "重命名…"), icon: Icons.rename, shortcut: "") {
                model.startRename(s)
            }
        case .note:
            return PanelAction(title: L.t("Write a Note…", "写便条…"), icon: Icons.note, shortcut: "",
                               help: L.t("Write a note on this session", "给这个会话写一句便条")) {
                model.editNote(s)
            }
        case .close:
            return PanelAction(title: L.t("Stop Reminding…", "不再提醒…"), icon: Icons.dismiss, shortcut: "") {
                model.askToClose([s])
            }
        }
    }

    /// cc-moss's own menu, on the logo: what isn't about one session.
    private var appSections: [[PanelAction]] {
        var first: [PanelAction] = []
        if model.phase == .ready {
            first.append(PanelAction(title: L.t("New Session…", "新会话…"), icon: Icons.newSession, shortcut: "⌘N") {
                model.startNewSession()
            })
        }
        first.append(PanelAction(title: L.t("Refresh", "刷新"), icon: Icons.refresh, shortcut: "⌘R") { model.refresh() })
        return [
            first,
            [PanelAction(title: L.t("Settings…", "设置…"), icon: Icons.settings, shortcut: "⌘,") { showSettings() }],
            [PanelAction(title: L.t("Quit cc-moss", "退出 cc-moss"), icon: Icons.quit, shortcut: "⌘Q") {
                NSApplication.shared.terminate(nil)
            }],
        ]
    }

    private func sections(for menu: PanelMenu) -> [[PanelAction]] {
        switch menu {
        case .app:
            return appSections
        case let .session(id, full):
            guard let s = model.sessions.first(where: { $0.id == id }) ?? model.moss.flatMap({ $0.id == id ? $0 : nil }) else { return [] }
            let actions = RowActions(s, hopeless: model.restoreIsHopeless(s))
            var sections = (full ? actions.all : actions.more).map { $0.map { action($0, for: s) } }
            // Moss is pinned already, above everything.
            if !s.isMoss { sections.append([pinAction(for: s)]) }
            return sections
        }
    }

    // MARK: Menus

    private var menuRow: String? {
        if case let .session(id, _) = menu { id } else { nil }
    }

    private func openMenu(_ next: PanelMenu, keyboard: Bool = false) {
        menuSections = sections(for: next)
        menuIndex = keyboard ? 0 : nil
        menu = next
    }

    private func toggleMenu(_ next: PanelMenu, keyboard: Bool = false) {
        if menu == next { closeMenu() } else { openMenu(next, keyboard: keyboard) }
    }

    private func closeMenu() {
        menu = nil
        menuIndex = nil
        menuSections = []
    }

    /// ⌘K: everything for the selected session, at its row.
    private func commandK() {
        if let s = model.selected, model.phase == .ready {
            toggleMenu(.session(s.id, full: true), keyboard: true)
        } else {
            toggleMenu(.app, keyboard: true)
        }
    }

    private func moveInMenu(_ step: Int) {
        let count = menuSections.joined().count
        guard count > 0 else { return }
        menuIndex = ((menuIndex ?? (step > 0 ? -1 : count)) + step + count) % count
    }

    private func runMenuItem() {
        let items = Array(menuSections.joined())
        guard let i = menuIndex, items.indices.contains(i) else { return }
        closeMenu()
        items[i].perform()
    }

    /// Under its row, right edges lined up (above it when there's no room
    /// below); cc-moss's own sits over the logo.
    private func menuOffset(_ menu: PanelMenu, sections: [[PanelAction]], row: CGRect?, in size: CGSize) -> CGSize {
        let height = ActionsMenu.height(sections), width = ActionsMenu.width
        let floor = size.height - 50 // the action bar
        guard case .session = menu, let row else {
            return CGSize(width: 10, height: floor - height)
        }
        let below = row.maxY - 2
        let y = below + height <= floor ? below : max(row.minY - height + 2, 8)
        return CGSize(width: min(row.maxX - width - 4, size.width - width - 8), height: y)
    }

    private func dismissForm() {
        // A message on its way can't be called back: Esc and the scrim wait.
        guard !model.sendFlow.isSending else { return }
        model.form = nil
        model.closeGate.cancel()
    }

    private func perform(_ action: PanelAction?) {
        closeMenu()
        action?.perform()
    }

    /// Open in the chosen terminal and get out of the way; if that fails,
    /// stay and say what to do instead.
    private func open(_ s: Session) {
        let panel = NSApplication.shared.keyWindow
        model.open(s) { opened in if opened { panel?.close() } }
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

    private func showMossSettings() {
        SettingsView.show(.moss)
        showSettings()
    }

    private func escape() {
        if model.form != nil || model.closeGate.asking {
            dismissForm()
        } else if menu != nil {
            closeMenu()
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
        case "k": commandK()
        case "r": model.refresh()
        case "p":
            if let s = model.selected, !s.isMoss, model.page == .sessions { model.togglePin(s) }
        case "n" where model.phase == .ready: model.startNewSession()
        case "e":
            // Say something to the session; one that can't take input gets a note instead.
            if let s = model.selected, model.phase == .ready {
                Message.allowed(s) ? model.writeMessage(s) : model.editNote(s)
            }
        case "q": NSApplication.shared.terminate(nil)
        case ",": showSettings()
        case "c" where press.modifiers.contains(.shift):
            if let s = model.selected { copy(s.state == .interrupted ? model.restoreCommand(s) : model.attachCommand(s)) }
        default: return .ignored
        }
        return .handled
    }
}

/// A menu in the panel: one session's ("⋯" shows what its row doesn't;
/// right-click and ⌘K show everything), or cc-moss's own on the logo.
enum PanelMenu: Equatable {
    case session(String, full: Bool)
    case app
}

/// Where the open session menu's row is, so the menu can sit under it.
struct MenuAnchorKey: PreferenceKey {
    static let defaultValue: Anchor<CGRect>? = nil
    static func reduce(value: inout Anchor<CGRect>?, nextValue: () -> Anchor<CGRect>?) {
        value = value ?? nextValue()
    }
}

struct PanelAction: Identifiable {
    /// Stable across redraws: a fresh id on every refresh made the row's
    /// buttons drop and reappear (a blink every few seconds).
    var id: String { "\(icon)|\(title)" }
    let title: String
    var icon: String = Icons.copy
    let shortcut: String
    /// Shown instead of `icon`: the chosen terminal's own app icon.
    var image: NSImage?
    var help: String?
    let perform: () -> Void
}

/// The line under a session's row for answering it in place: one line
/// (up to three), Claude Code's suggestion already in it and selected.
/// Return sends, ⇧Return makes a new line, Esc closes, ↓/↑ go to the next
/// or previous session waiting for you.
struct ReplyLineView: View {
    @Bindable var model: AppModel
    let session: Session
    var focused: FocusState<Bool>.Binding
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    private var sending: Bool { model.sendFlow.isSending }
    private var hasText: Bool { ReplyLine.toSend(model.replyDraft) != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .bottom, spacing: 6) {
                TextField(L.t("Reply…", "回一句…"), text: $model.replyDraft, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .lineLimit(1...3)
                    .focused(focused)
                    .disabled(sending)
                    // On its way: the words step back for "Sending…".
                    .opacity(sending ? 0 : 1)
                    .onChange(of: model.replyDraft) { _, text in model.replyEdited(text) }
                    .onKeyPress(.return, phases: .down) { press in
                        if press.modifiers.contains(.shift) {
                            NSApp.sendAction(#selector(NSResponder.insertNewlineIgnoringFieldEditor(_:)), to: nil, from: nil)
                        } else {
                            model.sendReply(session)
                        }
                        return .handled
                    }
                    .onKeyPress(.escape) { model.closeReply(); return .handled }
                    .onKeyPress(.downArrow) { move(backwards: false) }
                    .onKeyPress(.upArrow) { move(backwards: true) }
                    .padding(.horizontal, 9)
                    .padding(.vertical, 6)
                    .frame(minHeight: 30)
                    .background(RoundedRectangle(cornerRadius: 7, style: .continuous)
                        .fill(Color.primary.opacity(scheme == .dark ? 0.06 : 0.04)))
                    .overlay {
                        if sending {
                            ZStack {
                                HStack(spacing: 6) {
                                    ProgressView().controlSize(.mini)
                                    Text(L.t("Sending…", "发送中…"))
                                        .font(.system(size: 12))
                                        .foregroundStyle(.secondary)
                                }
                            }
                            .contentShape(Rectangle())
                            .transition(.opacity)
                        }
                    }
                Button { model.sendReply(session) } label: {
                    Image(systemName: Icons.message)
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(hasText && (hovering || !sending) ? AnyShapeStyle(Theme.brand) : AnyShapeStyle(.secondary))
                        .frame(width: 28, height: 30)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PressStyle())
                .focusable(false)
                .disabled(!hasText || sending)
                .allowsHitTesting(!sending)
                .onHover { hovering = $0 }
                .tip(L.t("Send  ↵", "发送  ↵"))
            }
            if let failure = model.sendFlow.failure {
                Text(failure)
                    .font(.system(size: 11))
                    .foregroundStyle(Theme.caption)
                    .lineLimit(2)
                    .padding(.leading, 2)
            }
        }
        // Under the session's name, inside the row's own background.
        .padding(.leading, 44)
        .padding(.trailing, 8)
        .padding(.bottom, 9)
        .animation(.easeOut(duration: 0.15), value: sending)
        .onAppear { focused.wrappedValue = true }
    }

    /// ↓/↑ go to the next session waiting for you, unless you're moving
    /// between lines of what you typed.
    private func move(backwards: Bool) -> KeyPress.Result {
        guard !model.replyDraft.contains("\n") else { return .ignored }
        model.moveReply(from: session, backwards: backwards)
        return .handled
    }
}
