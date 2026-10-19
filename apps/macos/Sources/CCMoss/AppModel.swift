import AppKit
import MossKit
import Observation

/// What the panel shows, refreshed from `cc-moss ls --json`.
@MainActor @Observable
final class AppModel {
    enum Phase: Equatable {
        case loading
        case ready
        /// No cc-moss binary in any of the usual places.
        case notFound
        /// cc-moss ran but something went wrong; the message is its own words.
        case failed(String)
        /// cc-moss is newer than this app.
        case needsUpgrade
    }

    var phase: Phase = .loading
    var sessions: [Session] = [] { didSet { syncIcon() } }
    /// Moss, kept apart from `sessions`: pinned on top of the list, never
    /// counted, restored or dismissed.
    var moss: Session? { didSet { if moss != oldValue { syncIcon() } } }

    /// The session with this id, Moss included (alerts name and open it).
    func session(_ id: String) -> Session? {
        sessions.first { $0.id == id } ?? moss.flatMap { $0.id == id ? $0 : nil }
    }
    /// The menu bar icon, following the sessions.
    @ObservationIgnored let icon = MenuBarIcon()
    var query = "" {
        didSet {
            guard query != oldValue else { return }
            selectFirst()
            // A search starts: read the closed sessions once, for its last section.
            if !searching { closedPool = nil } else if closedPool == nil { loadClosed() }
        }
    }
    /// A query is typed: the Sessions page is one ranked list.
    var searching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }
    /// Sessions closed in the last 30 days, read when a search starts.
    var closedPool: [Session]?
    @ObservationIgnored let searchIndexes = SearchIndexes()
    /// Set from the summary strip: show only sessions in this state.
    var stateFilter: SessionState? { didSet { if stateFilter != oldValue { selectFirst() } } }
    var selectedID: String?
    var refreshing = false

    @ObservationIgnored private(set) var client: MossClient?
    /// The cc-moss binary in use, for the settings window.
    var cliPath: String?
    @ObservationIgnored private var watcher: DirectoryWatcher?
    @ObservationIgnored private var watchedDir: String?
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var lastLoad = Date.distantPast
    @ObservationIgnored private var paced: Task<Void, Never>?
    @ObservationIgnored private lazy var flight = SingleFlight { [weak self] in await self?.load() }

    /// Which terminal sessions open in.
    @ObservationIgnored let terminals: TerminalPrefs
    /// Set when a session could not be opened; the panel shows what to do.
    var openProblem: OpenProblem?

    /// Bringing sessions back: what is in flight and what failed (T1.3).
    var restoreFlow = RestoreFlow()
    /// "Brought back N" on the banner, for a moment after bring-all-back.
    var restoreSummary: RestoreSummary?
    /// The "stopped earlier" group is open.
    var olderExpanded = false
    /// A short line in the action bar after something finished.
    var notice: String?

    /// The form floating over the panel: new session or a note (T2.5).
    var form: PanelForm? { didSet { if form != oldValue { sendFlow = .idle } } }
    /// A message to a session on its way (idle, sending, failed).
    var sendFlow = SendFlow()
    /// After a send, a note or a rename the session moves in the list: keep
    /// it selected and in view as the refreshed order arrives.
    @ObservationIgnored var reveal: (id: String, until: Date)?
    /// Bumped to scroll the selected row into view.
    var revealTick = 0
    /// The session whose row has its reply line open (one at a time).
    var replyOpenID: String?
    /// What's in that line.
    var replyDraft = ""
    /// A quiet note under one row for a moment (why it can't be typed into);
    /// `adopt`: it ends with "Move into tmux →".
    var rowNote: (id: String, text: String, adopt: Bool)?
    /// The row asking "Move into tmux?", and the name cc-moss said it'll get.
    var adoptAsk: AdoptAsk?
    /// Sessions being moved into tmux right now.
    var adopting: Set<String> = []
    /// Why a move into tmux didn't happen, by session id, shown on its row.
    var adoptProblems: [String: String] = [:]

    /// The Sessions page, flat: headings and rows, each row once, its pin
    /// taken from the section it's in.
    var listItems: [SessionListItem] {
        if searching { return SessionList.searchItems(query: query, live: searchHits, closed: closedHits) }
        return SessionList.items(pinned: pinnedShown, projects: projects, older: olderShown, olderExpanded: olderExpanded)
    }
    /// You typed in the line: a suggestion arriving later leaves it alone.
    /// Claude Code's suggestion in that line: quiet until you touch it.
    var replyField = SuggestionField()
    /// A newer cc-moss, when the last check found one (the gear's dot).
    var update: UpdateInfo?
    @ObservationIgnored var updateCheckedAt: Date?

    /// Look for a newer cc-moss when the panel opens, at most every 6 hours
    /// (it goes online, as Settings › About does). Quiet when it can't.
    func checkForUpdate() {
        guard let client, updateCheckedAt.map({ Date().timeIntervalSince($0) > 6 * 3600 }) ?? true else { return }
        updateCheckedAt = Date()
        Task {
            guard let info = try? await client.updateInfo() else { return }
            update = info.available ? info : nil
        }
    }

    /// The gear menu's "Update to …": cc-moss update, in the chosen terminal.
    func runUpdate() {
        guard let cliPath else { return }
        let command = LaunchPlan.cliCommand(cli: cliPath, ["update"])
        let terminal = terminals.current
        Task {
            if let failure = await Opener.run(command: command, in: terminal) {
                copy(failure.command)
                say(L.t("Couldn't open \(failure.terminalName). The command is on your clipboard.",
                        "没能打开\(failure.terminalName)。命令已经复制好了。"))
            }
        }
    }

    /// How tall the panel is: one of three sizes, picked as it opens.
    var panelHeight: CGFloat = Theme.panelHeight
    @ObservationIgnored var panelTier: PanelTier?

    /// The panel is about to open on a screen with `visible` points of height:
    /// pick its size from how many sessions you have (not Moss, not closed).
    func sizePanel(visible: CGFloat) -> CGFloat {
        let count = sessions.filter { $0.state != .closed }.count
        let tier = PanelSize.tier(count: count, previous: panelTier)
        panelTier = tier
        panelHeight = CGFloat(PanelSize.height(tier, visible: Double(visible)))
        return panelHeight
    }

    /// Sessions pinned to the top of the list (this Mac only).
    var pins = Pins.load()
    /// Off for screenshots: their pins are never saved.
    @ObservationIgnored var pinsPersist = true
    /// The session card in Moss's reply whose name field is open
    /// ("<message id>/<session id>").
    var mossCardRenaming: String?
    /// The session mention in Moss's words whose card is showing.
    var mentionHover: MentionHover?
    /// The mention under the pointer right now: lit at once, before its card.
    var mentionPointer = MossMentions.Pointer()
    @ObservationIgnored var mentionShow: Task<Void, Never>?
    @ObservationIgnored var mentionHide: Task<Void, Never>?
    /// Screenshots only: the suggestion every session "has".
    @ObservationIgnored var demoSuggestion: String?
    /// Claude Code's guess at what you'd say to Moss next (its own input box).
    var mossSuggestion: String?
    /// The badge for that guess was closed: hidden until Moss's next reply (its id).
    var mossGuessHiddenFor: String?
    /// What cc-moss suggest said per session and turn (memory only).
    @ObservationIgnored var suggestionCache = SuggestionCache()
    /// Screenshots only: Moss's suggestion, never asked of cc-moss.
    @ObservationIgnored var demoMossSuggestion: String?
    /// The new-session form's fields; kept here so they survive the panel
    /// closing while the folder chooser is open.
    var newDraft = NewDraft()
    /// "Stop reminding" waits for a second, deliberate click.
    var closeGate = Confirmation<[Session]>()

    /// Questions sessions are waiting on (T1.4), from `cc-moss ask list`,
    /// and the cards that answer them.
    var asks: [Ask] = []
    var askDesk = AskDesk()
    /// Everything waiting on you, worked through on the panel's "To Do" page.
    var askQueue = AskQueue()
    /// The panel page the side rail has picked.
    var page: PanelPage = .sessions {
        didSet {
            // Back on the Moss page later, it opens on its actions again.
            if page != .moss {
                mossTyping = false
                mossQuickOpen = false
            } else if oldValue != .moss {
                fetchMossSuggestion()
                // Opened again: the latest 30, every reply folded.
                mossLimit = MossPaging.page
                mossExpanded = []
            }
            if page == .moss && panelShown { mossDesk.markRead() }
        }
    }
    /// The panel is on screen (the Moss page counts as read only then).
    var panelShown = false {
        didSet { if panelShown && page == .moss { mossDesk.markRead() } }
    }
    /// Talking to Moss (T2.3): its status and the conversation.
    let mossDesk = MossDesk { _ in throw MossClient.Failure.command(L.t("cc-moss isn't installed yet.", "还没装 cc-moss。")) }
    /// The Moss page shows the text field instead of its action buttons.
    var mossTyping = false
    /// What's typed in the Moss page's field, not sent yet.
    var mossDraft = ""
    /// The quick bar's "⋯" list is open.
    var mossQuickOpen = false
    /// What the quick bar couldn't fit, for "⋯".
    var quickOverflow: [MossQuickItem] = []
    /// How many messages the Moss page reads (30, up to 100 with "Load earlier").
    var mossLimit = MossPaging.page
    /// Moss's replies you unfolded, while the page is open.
    var mossExpanded: Set<String> = []
    /// A Moss read is under way; another one waits for it instead of piling up.
    @ObservationIgnored var mossLoad: Task<Void, Never>?
    @ObservationIgnored var mossLoadAgain = false
    /// Screenshots only: the conversation opens scrolled to its top.
    @ObservationIgnored var mossStartAtTop = false
    /// How many messages the last read returned (before digests are hidden).
    var mossLoaded = 0
    /// What was sent from the quick bar and when, for its order (this Mac only).
    var quickUses: [MossQuickUse] = MossQuickLog.load()
    /// Off for screenshots: their made-up uses are never saved.
    @ObservationIgnored var quickPersist = true
    /// Bumped after every send: the quick bar re-ranks then (never while you hover).
    var quickSends = 0
    /// Moss answered something sent from the panel (for the alerts).
    @ObservationIgnored var onMossReplies: (([MossMessage]) -> Void)?
    /// Settings › Notifications › "Answer questions here" (`ask.enabled`).
    var askEnabled = true
    /// What changed in the question list, for the alerts.
    @ObservationIgnored var onAsks: ((AskDesk.Change) -> Void)?
    /// "The app is here": sessions wait a while for an answer from it.
    @ObservationIgnored var taker: AskTaker?

    /// `client` replaces the cc-moss lookup: demo screenshots pass a canned one.
    init(client: MossClient? = nil, terminals: TerminalPrefs? = nil) {
        self.terminals = terminals ?? TerminalPrefs()
        mossDesk.say = { [weak self] text in
            guard let client = self?.client else { throw MossClient.Failure.command(L.t("cc-moss isn't installed yet.", "还没装 cc-moss。")) }
            return try await client.mossSay(text)
        }
        if let client {
            self.client = client
            refresh()
        } else {
            start()
        }
    }

    var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "dev"
    }

    /// Projects after the search, in display order. Sessions that stopped
    /// long ago are left out: they fold into `olderShown`.
    var projects: [Project] {
        Filter.apply(query, state: stateFilter, to: Grouping.projects(RestoreGroups.listed(unpinned)))
    }

    /// Sessions you pinned, in pin order, after the search and the state filter.
    var pinnedShown: [Session] {
        pins.split(sessions).pinned.filter { Filter.keeps($0, label: $0.title, query: query, state: stateFilter) }
    }

    /// Everything not pinned: the folder groups and the older group.
    var unpinned: [Session] { pins.split(sessions).rest }

    /// Pin a session to the top of the list, or unpin it.
    func togglePin(_ s: Session) {
        pins.toggle(s.id)
        if pinsPersist { pins.save() }
    }

    /// Sessions in the order the arrow keys walk them: your pins, the
    /// folders, the folded group only while it is open. Never Moss: it has
    /// its own page (user 2026-10-19).
    var visible: [Session] {
        if searching { return searchHits.map(\.session) }
        return pinnedShown + projects.flatMap(\.sessions) + (olderExpanded ? olderShown : [])
    }

    /// Ranked results for the query, the state filter applied.
    var searchHits: [SearchHit] {
        let folders = Dictionary(Grouping.projects(sessions).flatMap { p in p.sessions.map { ($0.id, p.label) } },
                                 uniquingKeysWith: { a, _ in a })
        return searchIndexes.live(sessions, folders: folders).search(query)
            .filter { stateFilter == nil || $0.session.state == stateFilter }
    }

    /// Closed sessions that match (only without a state filter).
    var closedHits: [SearchHit] {
        guard stateFilter == nil, let closedPool else { return [] }
        return searchIndexes.closed(closedPool).search(query)
    }

    func loadClosed() {
        guard let client else { return }
        closedPool = []
        Task {
            guard let snap = try? await client.snapshotAll() else { return }
            let since = Date().addingTimeInterval(-30 * 86_400)
            closedPool = snap.sessions.filter { $0.state == .closed && !$0.isMoss && $0.lastActiveAt > since }
        }
    }

    /// Nothing found: ask Moss to look, on its page.
    func askMossToFind(_ q: String) {
        sayToMoss(L.t("Find me a session: \(q)", "帮我找一个会话：\(q)"))
        query = ""
        showMoss()
    }

    var selected: Session? {
        visible.first { $0.id == selectedID } ?? visible.first
    }

    func start() {
        // `cliPath` is the "cc-moss path" setting; empty means look in the
        // usual places.
        let override = UserDefaults.standard.string(forKey: "cliPath")
        guard let path = Locator(override: override).locate() else {
            cliPath = nil
            client = nil
            phase = .notFound
            return
        }
        cliPath = path
        // cc-moss answers in the app's language.
        var env = ProcessInfo.processInfo.environment
        env["CC_MOSS_LANG"] = L.lang == .zh ? "zh" : "en"
        client = MossClient(executable: path, runner: ProcessRunner(environment: env))
        startTaking()
        if timer == nil {
            // Hooks wake us through the watcher; this sweep finds sessions
            // whose process died without telling anyone.
            timer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
                Task { @MainActor in self?.refresh() }
            }
        }
        refresh()
    }

    /// The settings changed where cc-moss lives: look again.
    func relocate() {
        start()
    }

    func refresh() {
        if client == nil {
            // Installed since the app started?
            start()
            return
        }
        Task { await flight.request() }
    }

    /// Something worth seeing changed: refresh, but not sooner than
    /// `StateChange.gap` after the last one. Busy sessions write on every
    /// tool call; one refresh per gap catches them all.
    private func changed() {
        guard paced == nil else { return }
        let wait = lastLoad.addingTimeInterval(StateChange.gap(panelShown: panelShown)).timeIntervalSinceNow
        paced = Task { @MainActor [weak self] in
            if wait > 0 { try? await Task.sleep(for: .seconds(wait)) }
            self?.paced = nil
            self?.refresh()
        }
    }

    /// Load now and wait for it; for screenshots.
    func loadNow() async { await load() }

    private func load() async {
        guard let client else { return }
        refreshing = true
        lastLoad = Date()
        defer { refreshing = false }
        do {
            let snap = try await client.snapshot()
            let split = Grouping.splitMoss(snap.sessions)
            moss = split.moss
            sessions = split.others
            // Pins of sessions that are gone or closed go (saved with the next pin).
            let kept = pins.pruned(keeping: sessions)
            if kept != pins { pins = kept }
            restoreFlow.prune(keeping: Set(sessions.filter { $0.state == .interrupted }.map(\.id)))
            phase = .ready
            if let dir = snap.stateDir { watch(dir) }
            if let reveal, reveal.until > Date(), visible.contains(where: { $0.id == reveal.id }) {
                selectedID = reveal.id
                revealTick += 1
            } else if selectedID == nil || !visible.contains(where: { $0.id == selectedID }) {
                selectFirst()
            }
            await loadAsks()
            await loadMoss()
        } catch MossClient.Failure.newerSchema {
            phase = .needsUpgrade
        } catch MossClient.Failure.command(let message) {
            phase = .failed(message)
        } catch MossClient.Failure.timedOut {
            phase = .failed(L.t("cc-moss took too long to answer.", "cc-moss 太久没回话了。"))
        } catch {
            phase = .failed("\(error)")
        }
    }

    private func watch(_ dir: String) {
        guard dir != watchedDir else { return }
        watcher?.stop()
        watcher = DirectoryWatcher(path: dir, latency: 1.0, filter: { path in
            let announced = FileManager.default.fileExists(atPath: dir + "/registry.changed")
            return StateChange.matters(path, in: dir, announced: announced)
        }) { [weak self] in
            Task { @MainActor in self?.changed() }
        }
        watchedDir = dir
    }

    func selectFirst() { selectedID = visible.first?.id }

    /// Settings › General › Menu Bar: icon only, or icon and number.
    static let showCountKey = "menuBarShowsCount"

    func syncIcon() {
        icon.syncMotion()
        icon.update(Grouping.alerting(sessions, moss: moss),
                    showCount: UserDefaults.standard.bool(forKey: Self.showCountKey))
    }

    func move(_ delta: Int) {
        let list = visible
        guard !list.isEmpty else { return }
        let i = list.firstIndex { $0.id == selected?.id } ?? 0
        selectedID = list[min(max(i + delta, 0), list.count - 1)].id
    }

    /// cc-moss by absolute path, so commands work whatever the PATH.
    private var cli: String { client?.executable ?? "cc-moss" }

    /// The command that takes you into a session, for pasting into any terminal.
    func attachCommand(_ s: Session) -> String {
        LaunchPlan.attachCommand(cli: cli, handle: s.handle)
    }

    /// The command that brings an interrupted session back.
    func restoreCommand(_ s: Session) -> String {
        LaunchPlan.restoreCommand(cli: cli, handle: s.target)
    }

    /// Open `s` in the chosen terminal, off the main thread (AppleScript and
    /// `open` can take a while, or wait on a permission prompt). `done` gets
    /// true when it opened; otherwise `openProblem` says why.
    func open(_ s: Session, done: (@MainActor (Bool) -> Void)? = nil) {
        terminals.settle()
        let handle = s.handle, cli = cli, terminal = terminals.current, style = terminals.openStyle
        Task {
            let problem = await Opener.open(handle: handle, cli: cli, in: terminal, style: style)
            openProblem = problem
            done?(problem == nil)
        }
    }

    func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

/// The panel's pages, picked in its side rail.
enum PanelPage: Equatable {
    case sessions
    /// Questions and replies waiting on you (user 2026-10-18).
    case queue
    /// Moss's status and a conversation with it (T2.3).
    case moss
}

/// The search indexes, rebuilt only when the sessions change (pinyin isn't
/// free), never per keystroke.
final class SearchIndexes {
    private var liveKey: [String] = []
    private var liveIndex = SearchIndex([])
    private var closedKey: [String] = []
    private var closedIndex = SearchIndex([])

    private static func key(_ list: [Session]) -> [String] {
        list.map { "\($0.id)|\($0.title)|\($0.cwd)|\($0.lastActiveAt.timeIntervalSince1970)|\($0.lastPrompt ?? "")|\($0.notes.note ?? "")|\($0.state)" }
    }

    func live(_ sessions: [Session], folders: [String: String]) -> SearchIndex {
        let k = Self.key(sessions)
        if k != liveKey {
            liveKey = k
            liveIndex = SearchIndex(sessions, folders: folders)
        }
        return liveIndex
    }

    func closed(_ sessions: [Session]) -> SearchIndex {
        let k = Self.key(sessions)
        if k != closedKey {
            closedKey = k
            closedIndex = SearchIndex(sessions)
        }
        return closedIndex
    }
}
