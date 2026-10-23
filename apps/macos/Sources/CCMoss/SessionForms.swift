import AppKit
import MossKit
import SwiftUI

/// Which form floats over the panel.
enum PanelForm: Equatable {
    case newSession
    case note(Session)
    case message(Session)
    case rename(Session)
}

/// The new-session form's fields and what it is doing.
struct NewDraft: Equatable {
    var id = UUID()
    var directory = ""
    var name = ""
    /// The user edited the name: picking another folder leaves it alone.
    var nameEdited = false
    var working = false
    var directoryTouched = false
    var loadingCandidates = false
    var directorySelection = WorkingDirectorySelection()
    var candidates: [String] = []
    var workspaceDirectory: String?
    var attempted = false
    var receipt: NewSessionReceipt?
    var error: String?
    var checkingDirectory: Bool { loadingCandidates || directorySelection.pending != nil }
    var canSubmit: Bool { !working && !checkingDirectory && !attempted && !directory.isEmpty && NewSession.problem(with: name) == nil }
}

// MARK: - Model

extension AppModel {
    func startNewSession(workspaceOverride: String? = nil) {
        let workspace = workspaceOverride ?? UserDefaults.standard.string(forKey: NewSession.workspaceKey)
        newDraft = NewDraft()
        closeGate.cancel()
        endArchiveGate.cancel()
        form = .newSession
        guard let client, cliCapabilities.checksWorkingDirectory, cliCapabilities.returnsNewReceipt else {
            newDraft.error = L.t("Update cc-moss in Terminal before creating a session.", "请先更新终端里的 cc-moss，再新建会话。")
            return
        }
        var paths = NewSession.recentProjects(sessions, limit: 63)
        if let workspace, !workspace.isEmpty { paths = [workspace] + paths.filter { $0 != workspace } }
        guard !paths.isEmpty else { return }
        let draftID = newDraft.id
        let capabilities = cliCapabilities
        newDraft.loadingCandidates = true
        Task {
            do {
                let checks = try await client.checkDirectories(paths, capabilities: capabilities)
                guard form == .newSession, newDraft.id == draftID else { return }
                var seen = Set<String>()
                newDraft.candidates = checks.filter(\.approvedForNewSession).compactMap(\.canonical).filter { seen.insert($0).inserted }
                newDraft.workspaceDirectory = checks.first(where: { $0.path == workspace && $0.approvedForNewSession })?.canonical
                newDraft.loadingCandidates = false
                if !newDraft.directoryTouched {
                    if let first = newDraft.candidates.first {
                        newDraft.directory = first
                        if !newDraft.nameEdited { newDraft.name = NewSession.defaultName(for: first) }
                    } else {
                        newDraft.error = checks.first?.explanation ?? L.t("Choose a project folder to continue.", "请选择项目工作目录后再继续。")
                    }
                } else if !newDraft.directory.isEmpty, !newDraft.candidates.contains(newDraft.directory) {
                    newDraft.candidates.insert(newDraft.directory, at: 0)
                }
            } catch {
                guard form == .newSession, newDraft.id == draftID else { return }
                newDraft.loadingCandidates = false
                newDraft.error = Self.describe(error)
            }
        }
    }

    func pickDirectory(_ dir: String) {
        guard !newDraft.working, !newDraft.attempted else { return }
        newDraft.directoryTouched = true
        newDraft.directory = ""
        newDraft.error = nil
        guard let client, cliCapabilities.checksWorkingDirectory, !dir.isEmpty else {
            newDraft.error = WorkingDirectoryPolicy.updateRequired
            return
        }
        let draftID = newDraft.id
        let capabilities = cliCapabilities
        let ticket = newDraft.directorySelection.begin(dir)
        Task {
            do {
                let checks = try await client.checkDirectories([dir], capabilities: capabilities)
                guard form == .newSession, newDraft.id == draftID, let check = checks.first,
                      newDraft.directorySelection.complete(ticket, check: check) else { return }
                newDraft.directory = newDraft.directorySelection.canonical ?? ""
                newDraft.error = newDraft.directorySelection.error
                if let path = newDraft.directorySelection.canonical {
                    if !newDraft.candidates.contains(path) { newDraft.candidates.insert(path, at: 0) }
                    if !newDraft.nameEdited { newDraft.name = NewSession.defaultName(for: path) }
                }
            } catch {
                guard form == .newSession, newDraft.id == draftID else { return }
                newDraft.directorySelection.fail(ticket, message: Self.describe(error))
                newDraft.error = newDraft.directorySelection.error
            }
        }
    }

    func checkedTypedDirectory(_ query: String) async -> (option: DropdownOption<String>?, message: String?) {
        guard let path = FolderChoice.typedPath(query, home: NSHomeDirectory(), isFolder: { _ in true }) else { return (nil, nil) }
        guard let client, cliCapabilities.checksWorkingDirectory else { return (nil, WorkingDirectoryPolicy.updateRequired) }
        do {
            let checks = try await client.checkDirectories([path], capabilities: cliCapabilities)
            guard let check = checks.first, check.approvedForNewSession, let canonical = check.canonical else {
                return (nil, checks.first?.explanation ?? WorkingDirectoryReason.unavailable.explanation)
            }
            return (FolderChoice.option(canonical, symbol: Icons.project, badge: L.t("Checked", "已检查")), nil)
        } catch { return (nil, Self.describe(error)) }
    }

    /// "Another folder…": the system folder picker over the panel, starting
    /// at the current folder; the panel and the form stay as they were.
    func chooseDirectory(from window: NSWindow?) {
        let draftID = newDraft.id
        FilePicker.choose(.folder, from: window, startingAt: newDraft.directory,
                          message: L.t("Where should the new session work?", "新会话在哪个目录里干活？"),
                          prompt: L.t("Choose", "选择")) { [weak self] path in
            guard let self, self.form == .newSession, self.newDraft.id == draftID else { return }
            self.pickDirectory(path)
        }
    }

    /// One fresh launch, then only read-side tracking by its returned UUID.
    func submitNewSession() async -> Session? {
        guard !Task.isCancelled, form == .newSession, let client, newDraft.canSubmit else { return nil }
        guard cliCapabilities.checksWorkingDirectory && cliCapabilities.returnsNewReceipt else {
            newDraft.error = L.t("Update cc-moss in Terminal before creating a session.", "请先更新终端里的 cc-moss，再新建会话。")
            return nil
        }
        let draftID = newDraft.id
        let name = newDraft.name.trimmingCharacters(in: .whitespaces)
        let directory = newDraft.directory
        let capabilities = cliCapabilities
        newDraft.working = true
        newDraft.error = nil
        defer { if newDraft.id == draftID { newDraft.working = false } }
        do {
            let checks = try await client.checkDirectories([directory], capabilities: capabilities)
            guard !Task.isCancelled, form == .newSession, newDraft.id == draftID else { return nil }
            guard let check = checks.first, check.approvedForNewSession, let canonical = check.canonical else {
                newDraft.directory = ""
                newDraft.error = checks.first?.explanation ?? WorkingDirectoryReason.unavailable.explanation
                return nil
            }
            newDraft.directory = canonical
            newDraft.attempted = true
            let receipt = try await client.createNewSession(name: name, directory: canonical, capabilities: capabilities)
            guard form == .newSession, newDraft.id == draftID else { return nil }
            newDraft.receipt = receipt
            newDraft.name = receipt.name
            newDraft.working = false
            for _ in 0..<20 {
                guard form == .newSession, newDraft.id == draftID, !Task.isCancelled else { return nil }
                await loadNow()
                guard form == .newSession, newDraft.id == draftID else { return nil }
                if let registered = receipt.registeredSession(in: sessions) {
                    form = nil
                    return registered
                }
                try? await Task.sleep(for: .milliseconds(300))
            }
        } catch {
            guard form == .newSession, newDraft.id == draftID else { return nil }
            let notice = FailureNotice(action: .newSession, subject: .init(name: name, folder: directory), error: error)
            newDraft.error = notice.why
            report(notice)
        }
        // A sent request is never made again from this draft, including ambiguous transport errors.
        return nil
    }

    func editNote(_ s: Session) {
        closeGate.cancel()
        endArchiveGate.cancel()
        form = .note(s)
    }

    func writeMessage(_ s: Session) {
        closeGate.cancel()
        endArchiveGate.cancel()
        form = .message(s)
    }

    func startRename(_ s: Session) {
        closeGate.cancel()
        endArchiveGate.cancel()
        form = .rename(s)
    }

    /// Type `text` into the session, as if in its terminal. One at a time:
    /// the form stays, locked, until it went; on failure it keeps the text.
    func sendMessage(_ s: Session, text: String, done: String) {
        guard let client, sendFlow.begin() else { return }
        Task {
            do {
                try await client.send(target: s.target, text: text)
                form = nil
                sendFlow.finish(error: nil)
                say(done)
                follow(s)
            } catch {
                sendFlow.finish(error: Self.describe(error))
                report(.send, s, error: error)
            }
            refresh()
        }
    }

    // MARK: Reply lines (user 2026-10-19)

    /// A click on a session waiting for you: a reply line opens under its
    /// row, with Claude Code's own suggestion already in it. Any other row
    /// closes it.
    func openReply(_ s: Session) {
        // Another row: its "Move into tmux?" goes away.
        if adoptAsk?.id != s.id { adoptAsk = nil }
        guard ReplyLine.opens(s) else {
            closeReply()
            // Not a silent no-op: say why, under the row, for a moment.
            if let why = ReplyLine.blocked(s) { showRowNote(s.id, why) }
            return
        }
        guard replyOpenID != s.id else { return }
        guard !replyFlow.isSending else { return }
        replyOpenID = s.id
        replyDraft = ""
        replyField = SuggestionField()
        replyFlow = .idle
        // Mid-turn Claude Code shows no suggestion: nothing to ask for.
        guard !ReplyLine.busy(s) else { return }
        let handle = s.handle
        Task {
            let guess = if let demo = demoSuggestion { demo } else { await suggestion(for: s) }
            guard replyOpenID == s.id, let fill = replyField.offer(guess, text: replyDraft) else { return }
            replyDraft = fill
            FieldSelection.selectAllQuietly()
        }
    }

    /// The line's text changed: yours, unless it's the suggestion going in.
    func replyEdited(_ text: String) {
        let was = replyField.untouched
        replyField.changed(to: text)
        if was && !replyField.untouched { FieldSelection.showHighlight() }
    }

    /// An arrow key or a click in the line: the suggestion is ordinary text now.
    func replyEngaged() {
        guard replyField.untouched else { return }
        replyField.engaged()
        FieldSelection.showHighlight()
    }

    /// A quiet line under a row for 2 s: why it can't be typed into.
    func showRowNote(_ id: String, _ why: ReplyLine.Blocked) {
        // Outside tmux and running: the way in is right there.
        let movable = why == .notInTmux && sessions.first(where: { $0.id == id }).map { Adopt.availability($0) != .none } == true
        let text = switch why {
        case .notInTmux where movable: L.t("Not in tmux", "不在 tmux 里")
        case .notInTmux: L.t("Not in tmux, so it can't be typed into from here", "不在 tmux 里，没法从这里打字")
        case .dropped: L.t("Dropped; bring it back first", "断了，先找回")
        }
        showRowNote(id, text: text, adopt: movable)
    }

    /// A quiet line under a row for a moment; one with a link stays longer.
    func showRowNote(_ id: String, text: String, adopt: Bool = false) {
        rowNote = (id, text, adopt)
        Task {
            try? await Task.sleep(nanoseconds: adopt ? 5_000_000_000 : 2_000_000_000)
            if rowNote?.id == id && rowNote?.text == text { rowNote = nil }
        }
    }

    func closeReply() {
        guard !replyFlow.isSending else { return }
        replyOpenID = nil
    }

    /// Return in the line: send what's in it. Then on to the next session
    /// waiting for you, its line open, so a run of replies is just Return.
    func sendReply(_ s: Session) {
        guard let text = ReplyLine.toSend(replyDraft), let client, replyFlow.begin() else { return }
        Task {
            do {
                try await client.send(target: s.target, text: text)
                replyFlow.finish(error: nil)
                say(L.t("Sent to \(s.title).", "已发给 \(s.title)。"))
                let next = ReplyLine.next(after: s.id, in: visible)
                replyOpenID = nil
                if let next {
                    selectedID = next.id
                    revealTick += 1
                    openReply(next)
                } else {
                    follow(s)
                }
            } catch {
                replyFlow.finish(error: Self.describe(error))
                report(.send, s, error: error)
            }
            refresh()
        }
    }

    /// ↓/↑ in the line: the next (or previous) session waiting for you.
    func moveReply(from s: Session, backwards: Bool) {
        guard !replyFlow.isSending, let n = ReplyLine.next(after: s.id, in: visible, backwards: backwards) else { return }
        selectedID = n.id
        revealTick += 1
        openReply(n)
    }

    /// The session just written to will move up the list: keep it selected
    /// and in view for the next few refreshes.
    func follow(_ s: Session) {
        selectedID = s.id
        reveal = (s.id, Date().addingTimeInterval(6))
        revealTick += 1
    }

    func saveNote(_ s: Session, text: String) {
        guard let client else { return }
        form = nil
        Task {
            do {
                try await client.setNote(target: s.target, text: text)
                say(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    ? L.t("Note cleared.", "便条已清除。")
                    : L.t("Noted.", "记下了。"))
                follow(s)
            } catch {
                report(.note, s, error: error)
            }
            refresh()
        }
    }

    /// First click of "Stop reminding": ask.
    func askToClose(_ list: [Session]) {
        let allowed = list.filter(Close.allowed)
        guard !allowed.isEmpty else { return }
        form = nil
        endArchiveGate.cancel()
        closeGate.request(allowed)
    }

    /// Second click: do it.
    func confirmClose() {
        if let list = closeGate.confirm() { close(list) }
    }
}

// MARK: - Shared look

/// A form card floating over the panel, in the settings' measures
/// (2.5% tint rows, 12 pt insets) on the raised surface.
struct FormCard<Content: View>: View {
    let title: String
    let icon: String
    /// A quiet fact about the form, at the right of the title (where a
    /// note goes, how a message arrives) instead of a sentence under it.
    var aside: (icon: String, text: String)?
    var flexible = false
    @ViewBuilder let content: () -> Content
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                Text(title).font(.system(size: 14, weight: .semibold)).lineLimit(1)
                Spacer(minLength: 8)
                if let aside {
                    HStack(spacing: 4) {
                        Image(systemName: aside.icon).font(.system(size: 10, weight: .semibold))
                        Text(aside.text).font(.system(size: 11))
                    }
                    .foregroundStyle(Theme.caption)
                    .fixedSize()
                }
            }
            .padding(.horizontal, 4)
            content()
        }
        .padding(12)
        .frame(width: flexible ? nil : Theme.pageWidth - 28)
        .frame(maxWidth: flexible ? .infinity : nil)
        .background(Theme.raised(scheme), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Theme.hairline(scheme)))
        .shadow(color: .black.opacity(0.18), radius: 16, y: 6)
    }
}

/// One labelled row inside a form card.
struct FormRow<Control: View>: View {
    let title: String
    var divider = true
    @ViewBuilder let control: () -> Control

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Text(title).font(.system(size: 13)).foregroundStyle(.secondary)
                    .frame(width: 52, alignment: .leading)
                control()
            }
            .padding(.horizontal, 12)
            .frame(minHeight: 46)
            if divider {
                Rectangle().fill(Color.primary.opacity(0.05)).frame(height: 1).padding(.horizontal, 12)
            }
        }
    }
}

/// The hint or error line under a form.
struct FormHint: View {
    let text: String
    var icon: String?
    var tint: Color = .secondary

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 5) {
            if let icon {
                Image(systemName: icon).font(.system(size: 10, weight: .semibold)).foregroundStyle(tint)
            }
            Text(text).font(.system(size: 11)).foregroundStyle(Theme.caption)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 4)
    }
}

// MARK: - New session

struct NewSessionForm: View {
    @Bindable var model: AppModel
    /// Open this session in the terminal (and close the panel).
    let open: (Session) -> Void

    private var draft: NewDraft { model.newDraft }

    var body: some View {
        let draftID = draft.id
        FormCard(title: L.t("New Session", "新会话"), icon: Icons.newSession, flexible: true) {
            VStack(spacing: 0) {
                FormRow(title: L.t("Folder", "目录")) {
                    Dropdown(options: folderOptions, selection: draft.directory, pick: { id in
                        guard model.form == .newSession, model.newDraft.id == draftID else { return }
                        if id == Self.chooseID {
                            model.chooseDirectory(from: NSApp.keyWindow)
                        } else {
                            model.pickDirectory(id)
                        }
                    }, listWidth: 380, placeholder: L.t("Search, or type a path…", "搜索，或输入路径…"),
                       checkedTyped: { await model.checkedTypedDirectory($0) }, fixedLabel: false) {
                        let label = draft.directory.isEmpty
                            ? FolderChoice.Label(name: L.t("Choose working folder", "选择工作目录"), parent: nil)
                            : FolderChoice.label(draft.directory, home: NSHomeDirectory())
                        HStack(spacing: 6) {
                            Image(systemName: Icons.project).font(.system(size: 11)).foregroundStyle(.secondary)
                            Text(label.name).layoutPriority(1)
                            if let parent = label.parent {
                                Text(parent).foregroundStyle(.secondary).truncationMode(.middle)
                            }
                        }
                        .frame(maxWidth: 240, alignment: .leading)
                    }
                    .tip(draft.directory)
                    Spacer(minLength: 0)
                }
                FormRow(title: L.t("Name", "名字"), divider: false) {
                    FilledField(placeholder: L.t("e.g. login-page", "比如 登录页"),
                                // Only a real change counts as editing: the field writes its
                                // value back when it takes the keyboard again.
                                text: Binding(get: { model.newDraft.name },
                                              set: { new in
                                                  guard new != model.newDraft.name else { return }
                                                  model.newDraft.name = new; model.newDraft.nameEdited = true
                                                  model.newDraft.error = nil }),
                                autofocus: true,
                                tint: nameClash,
                                onSubmit: submit)
                }
            }
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.primary.opacity(0.025)))
            .disabled(draft.working || draft.attempted)
            hint
            footer
        }
    }

    static let chooseID = "\u{0}choose"

    private func folderOptions() -> [DropdownOption<String>] {
        var options = draft.candidates.map { dir in
            dir == draft.workspaceDirectory
                ? FolderChoice.option(dir, symbol: "star", badge: L.t("Workspace", "工作区"))
                : FolderChoice.option(dir, symbol: Icons.project)
        }
        options.append(DropdownOption(id: Self.chooseID, title: L.t("Another folder…", "其他文件夹…"), symbol: Icons.chooseFolder))
        return options
    }

    @ViewBuilder private var hint: some View {
        if let error = draft.error {
            FormHint(text: error, icon: Icons.warning, tint: Theme.amber)
        } else if let receipt = draft.receipt {
            FormHint(text: receipt.statusMessage(in: model.sessions))
        } else if draft.checkingDirectory {
            FormHint(text: L.t("Checking working folders…", "正在检查工作目录…"))
        } else {
            switch NewSession.hint(name: draft.name, sessions: model.sessions) {
            case .free:
                FormHint(text: L.t("Starts in tmux and opens in your terminal.", "在 tmux 里开起来，再在你的终端里打开。"))
            case .running, .interrupted:
                FormHint(text: L.t("This name is in use. A new session gets its own numbered name.", "已有同名会话，新会话会自动加上编号，不会打开或找回旧会话。"))
            case .invalid(let why):
                FormHint(text: why, icon: Icons.warning, tint: Theme.amber)
            }
        }
    }

    /// The name is taken: say what Return does now, and offer a free name in one click.
    private func clash(_ text: String, icon: String, tint: Color) -> some View {
        let free = NewSession.freeName(draft.name, sessions: model.sessions)
        return VStack(alignment: .leading, spacing: 6) {
            FormHint(text: text, icon: icon, tint: tint)
            SuggestionChip(text: free, chosen: false) {
                model.newDraft.name = free
                model.newDraft.nameEdited = true
            }
            .padding(.leading, 19)
        }
    }

    /// The field's outline while the name belongs to another session.
    private var nameClash: Color? {
        switch NewSession.hint(name: draft.name, sessions: model.sessions) {
        case .running: return Theme.brand
        case .interrupted: return Theme.red
        case .free, .invalid: return nil
        }
    }

    private var footer: some View {
        HStack(spacing: 8) {
            if draft.working {
                ProgressView().controlSize(.small).scaleEffect(0.7)
                Text(L.t("Starting…", "正在开…")).font(.system(size: 12)).foregroundStyle(.secondary)
            }
            Spacer()
            SoftButton(title: draft.attempted ? L.t("Dismiss", "收起") : L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.form = nil }
            SoftButton(title: primaryTitle, icon: primaryIcon, prominent: true, shortcut: "↵", action: submit)
                .disabled(!primaryEnabled)
                .opacity(primaryEnabled ? 1 : 0.45)
        }
    }

    private var registered: Session? { draft.receipt?.registeredSession(in: model.sessions) }
    private var primaryEnabled: Bool { registered != nil || (draft.canSubmit && model.cliCapabilities.returnsNewReceipt && model.cliCapabilities.checksWorkingDirectory) }
    private var primaryTitle: String {
        if registered != nil { return L.t("Open", "打开") }
        if draft.receipt != nil { return L.t("Waiting", "等待登记") }
        return L.t("Create", "新建")
    }
    private var primaryIcon: String { registered == nil ? Icons.newSession : Icons.open }

    private func submit() {
        if let registered { model.form = nil; open(registered); return }
        Task {
            if let s = await model.submitNewSession() { open(s) }
        }
    }
}

// MARK: - Note

struct NoteForm: View {
    @Bindable var model: AppModel
    let session: Session
    @State private var text: String

    init(model: AppModel, session: Session) {
        self.model = model
        self.session = session
        _text = State(initialValue: Note.current(session))
    }

    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    private var hadNote: Bool { !Note.current(session).isEmpty }
    private var clears: Bool { empty && hadNote }

    var body: some View {
        FormCard(title: L.t("Note for “\(session.title)”", "给「\(session.title)」写便条"), icon: Icons.note,
                 aside: ("lock", L.t("Claude doesn't see it", "Claude 看不到"))) {
            MultilineField(placeholder: L.t("Where it stopped, what's next…", "停在哪里、下一步做什么…"),
                           text: $text) { model.saveNote(session, text: text) }
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.form = nil }
                // Emptied, saving clears the note: say so on the button.
                SoftButton(title: clears ? L.t("Clear Note", "清除便条") : L.t("Save", "保存"),
                           icon: clears ? "trash" : "checkmark", prominent: true, shortcut: "⌘↩") {
                    model.saveNote(session, text: text)
                }
                .disabled(empty && !hadNote)
            }
        }
    }
}

// MARK: - Message

/// Say something to the session: the text goes into its input, as if typed
/// in its terminal; several lines arrive whole.
struct MessageForm: View {
    @Bindable var model: AppModel
    let session: Session
    @State private var text = ""
    /// Claude Code's suggestion in the field: quiet until you touch it.
    @State private var field = SuggestionField()

    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        FormCard(title: L.t("Message to “\(session.title)”", "给「\(session.title)」发消息"), icon: Icons.message,
                 aside: ("terminal", L.t("Typed into it", "像在终端里打字"))) {
            MultilineField(placeholder: L.t("What should Claude do next?", "想让 Claude 接下来做什么？"), text: $text,
                           locked: sending, quiet: field.untouched, onEngage: engage) { send() }
                .onChange(of: text) { _, new in
                    let was = field.untouched
                    field.changed(to: new)
                    if was && !field.untouched { FieldSelection.showHighlight() }
                }
                .onAppear { if let demo = model.demoSuggestion { take(demo) } }
                .task(id: session.id) {
                    // Claude Code's own suggestion, asked once, off the main
                    // thread, quick or not at all; it goes straight into the field.
                    guard model.demoSuggestion == nil else { return }
                    take(await model.suggestion(for: session))
                }
            if let failure = model.sendFlow.failure {
                HStack(spacing: 5) {
                    Image(systemName: "exclamationmark.circle.fill").font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Theme.red)
                    Text(failure).font(.system(size: 11)).foregroundStyle(Theme.caption)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.leading, 2)
            }
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.form = nil }
                    .disabled(sending)
                    .opacity(sending ? 0.5 : 1)
                // Locked while sending, but still the Send button.
                SoftButton(title: L.t("Send", "发送"), icon: Icons.message, prominent: true, shortcut: "⌘↩") { send() }
                    .disabled(empty || sending)
                    .allowsHitTesting(!sending)
            }
        }
    }

    private var sending: Bool { model.sendFlow.isSending }

    /// The suggestion goes in as real text, all selected, unless you've typed.
    private func take(_ guess: String?) {
        guard let fill = field.offer(guess, text: text) else { return }
        text = fill
        FieldSelection.selectAllQuietly()
    }

    private func engage() {
        guard field.untouched else { return }
        field.engaged()
        FieldSelection.showHighlight()
    }

    private func send() {
        guard !empty, !sending else { return }
        model.sendMessage(session, text: text, done: L.t("Sent to \(session.title).", "已发给 \(session.title)。"))
    }
}

// MARK: - Rename

/// A new name, set the way Claude Code names sessions (`/rename`), so the
/// hook carries it to tmux and the panel; names stay unique.
struct RenameForm: View {
    @Bindable var model: AppModel
    let session: Session
    @State private var name: String

    init(model: AppModel, session: Session) {
        self.model = model
        self.session = session
        _name = State(initialValue: session.name ?? "")
    }

    private var check: Message.RenameCheck { Message.checkRename(name, of: session, among: model.sessions) }

    /// What the field says now, with its colour: fine, or what to change.
    private var status: (icon: String, text: String, tint: Color) {
        switch check {
        case .ok:
            return ("checkmark.circle.fill", L.t("Claude Code renames it; the panel and tmux follow.",
                                                 "由 Claude Code 改名，面板和 tmux 跟着变。"), .green)
        case .unchanged:
            return ("equal.circle", L.t("That's its name already.", "现在就叫这个。"), .secondary)
        case .empty:
            return ("pencil.circle", L.t("Type a name, or pick one below.", "输入新名字，或点一个建议。"), .secondary)
        case .badCharacters:
            return ("exclamationmark.circle.fill", L.t("One line of plain text, please.", "只能是一行普通文字。"), Theme.amber)
        case .taken(let other):
            return ("exclamationmark.circle.fill", L.t("“\(other)” has this name already.", "「\(other)」已经叫这个名字了。"), Theme.amber)
        }
    }

    private var suggestions: [String] { Message.renameSuggestions(for: session, among: model.sessions) }

    var body: some View {
        FormCard(title: L.t("Rename “\(session.title)”", "给「\(session.title)」改名"), icon: Icons.rename) {
            FilledField(placeholder: L.t("New name", "新名字"), text: $name, autofocus: true, large: true) { submit() }
            if !suggestions.isEmpty {
                HStack(spacing: 6) {
                    Text(L.t("Try", "试试"))
                        .font(.system(size: 11))
                        .foregroundStyle(Theme.caption)
                    ForEach(suggestions, id: \.self) { s in
                        SuggestionChip(text: s, chosen: name == s) { name = s }
                    }
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 2)
            }
            let st = status
            HStack(spacing: 5) {
                Image(systemName: st.icon).font(.system(size: 11, weight: .semibold)).foregroundStyle(st.tint)
                Text(st.text).font(.system(size: 11)).foregroundStyle(Theme.caption)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.leading, 2)
            .animation(.easeOut(duration: 0.12), value: st.text)
            HStack(spacing: 8) {
                Spacer(minLength: 8)
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.form = nil }
                SoftButton(title: L.t("Rename", "改名"), icon: Icons.rename, prominent: true, shortcut: "↵") { submit() }
                    .disabled(check != .ok)
            }
            .padding(.leading, 2)
        }
    }

    private func submit() {
        guard check == .ok, !model.sendFlow.isSending else { return }
        let new = name.trimmingCharacters(in: .whitespaces)
        model.sendMessage(session, text: Message.renameText(new),
                          done: L.t("Renaming to \(new)…", "正在改名为 \(new)…"))
    }
}

/// A few lines of text in the panel's filled style: ⌘↩ submits, Return
/// makes a new line, Esc is handled by the form.
struct MultilineField: View {
    let placeholder: String
    @Binding var text: String
    /// On its way: covered by "Sending…", no typing, no focus.
    var locked = false
    /// A suggestion nobody has touched yet: grey, like a placeholder.
    var quiet = false
    /// An arrow key or a click in the text.
    var onEngage: () -> Void = {}
    var onSubmit: () -> Void = {}
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack(alignment: .topLeading) {
            if text.isEmpty {
                Text(placeholder)
                    .font(.system(size: 13))
                    .foregroundStyle(.tertiary)
                .padding(.horizontal, 10)
                .padding(.vertical, 8)
                .allowsHitTesting(false)
            }
            TextEditor(text: $text)
                .font(.system(size: 13))
                .foregroundStyle(quiet ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.primary))
                .simultaneousGesture(TapGesture().onEnded { onEngage() })
                .onKeyPress(keys: [.leftArrow, .rightArrow, .upArrow, .downArrow]) { _ in onEngage(); return .ignored }
                .scrollContentBackground(.hidden)
                .focused($focused)
                .padding(.horizontal, 5)
                .padding(.vertical, 6)
                .onKeyPress(.return, phases: .down) { press in
                    guard press.modifiers.contains(.command) else { return .ignored }
                    onSubmit()
                    return .handled
                }
        }
        .frame(minHeight: 84, maxHeight: 150)
        .background(RoundedRectangle(cornerRadius: 8).fill(Theme.selection(scheme).opacity(0.8)))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .strokeBorder(focused && !locked ? Theme.brand.opacity(0.7) : Theme.hairline(scheme), lineWidth: focused && !locked ? 1.5 : 1)
        )
        .disabled(locked)
        .overlay {
            if locked {
                ZStack {
                    RoundedRectangle(cornerRadius: 8).fill(Theme.raised(scheme).opacity(0.94))
                    RoundedRectangle(cornerRadius: 8).strokeBorder(Theme.hairline(scheme))
                    HStack(spacing: 7) {
                        ProgressView().controlSize(.small)
                        Image(systemName: Icons.message)
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(Theme.brand)
                        Text(L.t("Sending…", "发送中…"))
                            .font(.system(size: 12.5, weight: .medium))
                            .foregroundStyle(.secondary)
                    }
                }
                .contentShape(Rectangle()) // takes the clicks: nothing reaches the field
                .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.12), value: focused)
        .animation(.easeOut(duration: 0.15), value: locked)
        .onChange(of: locked) { _, now in if now { focused = false } else { focused = true } }
        .onAppear { focused = true }
    }
}

/// Text put into a field for you (a suggestion) arrives selected, so typing
/// replaces it, but without the selection's highlight: it should look as
/// quiet as a placeholder (user 2026-10-19).
enum FieldSelection {
    @MainActor static func selectAllQuietly() {
        // Once the field has the new text and the keyboard.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            guard let editor = NSApp.keyWindow?.firstResponder as? NSTextView else { return }
            editor.selectedTextAttributes = [.backgroundColor: NSColor.clear]
            editor.selectAll(nil)
        }
    }

    /// The text is yours now: selections look like selections again.
    @MainActor static func showHighlight() {
        guard let editor = NSApp.keyWindow?.firstResponder as? NSTextView else { return }
        editor.selectedTextAttributes = [.backgroundColor: NSColor.selectedTextBackgroundColor,
                                         .foregroundColor: NSColor.selectedTextColor]
    }
}

// MARK: - Stop reminding

struct CloseConfirmCard: View {
    @Bindable var model: AppModel
    let sessions: [Session]

    var body: some View {
        FormCard(title: Close.question(sessions), icon: Icons.dismiss) {
            FormHint(text: L.t("No more restore prompts. `cc-moss ls --all` still lists it.",
                               "以后不再提示找回，`cc-moss ls --all` 里还能看到。"))
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.closeGate.cancel() }
                SoftButton(title: L.t("Stop Reminding", "不再提醒"), icon: Icons.dismiss, prominent: true, shortcut: "↵") {
                    model.confirmClose()
                }
            }
        }
    }
}

/// The dimmed layer under a form: a click outside cancels it.
struct FormScrim: View {
    let dismiss: () -> Void
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Rectangle()
            .fill(Color.black.opacity(scheme == .dark ? 0.35 : 0.12))
            .contentShape(Rectangle())
            .onTapGesture(perform: dismiss)
            .accessibilityHidden(true)
    }
}

/// The "+" in the footer: start a new session (⌘N).
struct NewSessionButton: View {
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 5) {
                Image(systemName: Icons.newSession)
                    .font(.system(size: 12, weight: .semibold))
                Text(L.t("New Session", "新会话"))
                    .font(.system(size: 12, weight: .medium))
                    .lineLimit(1)
                    .fixedSize()
            }
            .foregroundStyle(hovering ? .primary : .secondary)
            .padding(.horizontal, 8)
            .frame(height: 26)
            .background(RoundedRectangle(cornerRadius: 7).fill(hovering ? Color.primary.opacity(0.06) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .onHover { inside in withAnimation(.easeOut(duration: 0.12)) { hovering = inside } }
        .tip(L.t("New Session  ⌘N", "新会话  ⌘N"))
        .accessibilityLabel(L.t("New Session", "新会话"))
    }
}

/// A name to pick with one click: a small capsule, filled when it is the
/// field's text.
struct SuggestionChip: View {
    let text: String
    let chosen: Bool
    let action: () -> Void
    @State private var hovering = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Button(action: action) {
            Text(text)
                .font(.system(size: 12))
                .lineLimit(1)
                .foregroundStyle(chosen ? Theme.brand : .primary)
                .padding(.horizontal, 9)
                .frame(height: 22)
                .background(Capsule().fill(chosen ? Theme.brand.opacity(0.12)
                                                  : Color.primary.opacity(hovering ? 0.09 : 0.05)))
                .overlay(Capsule().strokeBorder(chosen ? Theme.brand.opacity(0.35) : .clear))
                .contentShape(Capsule())
        }
        .buttonStyle(PressStyle())
        .focusable(false)
        .handCursor()
        .onHover { inside in withAnimation(.easeOut(duration: 0.1)) { hovering = inside } }
    }
}
