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
    var query = "" { didSet { if query != oldValue { selectFirst() } } }
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
        Filter.apply(query, state: stateFilter, to: Grouping.projects(RestoreGroups.listed(sessions)))
    }

    /// Moss's pinned row, unless the search or the state filter hides it.
    var pinnedMoss: Session? {
        guard let moss, Filter.keeps(moss, label: "Moss", query: query, state: stateFilter) else { return nil }
        return moss
    }

    /// Sessions in the order the arrow keys walk them (Moss first, the
    /// folded group only while it is open).
    var visible: [Session] {
        (pinnedMoss.map { [$0] } ?? []) + projects.flatMap(\.sessions) + (olderExpanded ? olderShown : [])
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
