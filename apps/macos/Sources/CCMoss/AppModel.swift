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
    var sessions: [Session] = []
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
    @ObservationIgnored private lazy var flight = SingleFlight { [weak self] in await self?.load() }

    /// Which terminal sessions open in.
    @ObservationIgnored let terminals: TerminalPrefs
    /// Set when a session could not be opened; the panel shows what to do.
    var openProblem: OpenProblem?

    /// `client` replaces the cc-moss lookup: demo screenshots pass a canned one.
    init(client: MossClient? = nil, terminals: TerminalPrefs? = nil) {
        self.terminals = terminals ?? TerminalPrefs()
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

    /// Projects after the search, in display order.
    var projects: [Project] {
        Filter.apply(query, state: stateFilter, to: Grouping.projects(sessions))
    }

    /// Sessions in the order the arrow keys walk them.
    var visible: [Session] { projects.flatMap(\.sessions) }

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

    /// Load now and wait for it; for screenshots.
    func loadNow() async { await load() }

    private func load() async {
        guard let client else { return }
        refreshing = true
        defer { refreshing = false }
        do {
            let snap = try await client.snapshot()
            sessions = snap.sessions
            phase = .ready
            if let dir = snap.stateDir { watch(dir) }
            if selectedID == nil || !visible.contains(where: { $0.id == selectedID }) { selectFirst() }
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
        watcher = DirectoryWatcher(path: dir, latency: 1.0) { [weak self] in
            Task { @MainActor in self?.refresh() }
        }
        watchedDir = dir
    }

    func selectFirst() { selectedID = visible.first?.id }

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
        LaunchPlan.restoreCommand(cli: cli, handle: s.handle)
    }

    /// Open `s` in the chosen terminal. True when it opened; otherwise
    /// `openProblem` says why.
    func open(_ s: Session) -> Bool {
        terminals.settle()
        openProblem = Opener.open(handle: s.handle, cli: cli, in: terminals.current)
        return openProblem == nil
    }

    func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}
