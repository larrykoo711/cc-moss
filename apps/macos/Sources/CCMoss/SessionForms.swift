import AppKit
import MossKit
import SwiftUI

/// Which form floats over the panel.
enum PanelForm: Equatable {
    case newSession
    case note(Session)
}

/// The new-session form's fields and what it is doing.
struct NewDraft: Equatable {
    var directory = ""
    var name = ""
    /// The user edited the name: picking another folder leaves it alone.
    var nameEdited = false
    /// The name belongs to an interrupted session; asking "bring it back?".
    var restoring: Session?
    var working = false
    var error: String?
}

// MARK: - Model

extension AppModel {
    func startNewSession() {
        let dir = NewSession.defaultDirectory(sessions) ?? NSHomeDirectory()
        newDraft = NewDraft(directory: dir, name: NewSession.defaultName(for: dir))
        closeGate.cancel()
        form = .newSession
    }

    func pickDirectory(_ dir: String) {
        newDraft.directory = dir
        newDraft.error = nil
        if !newDraft.nameEdited { newDraft.name = NewSession.defaultName(for: dir) }
    }

    /// "Choose…": the system folder picker, starting at the current folder.
    func chooseDirectory() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true
        panel.directoryURL = URL(fileURLWithPath: newDraft.directory)
        panel.message = L.t("Where should the new session work?", "新会话在哪个目录里干活？")
        panel.prompt = L.t("Choose", "选择")
        if panel.runModal() == .OK, let url = panel.url { pickDirectory(url.path) }
    }

    /// Create (or bring back) the session in the form. Returns the session
    /// to open once cc-moss lists it; nil when there is nothing to open.
    func submitNewSession() async -> Session? {
        guard let client, !newDraft.working else { return nil }
        let name = newDraft.name.trimmingCharacters(in: .whitespaces)
        let args: [String]
        if let stopped = newDraft.restoring {
            // The user said yes to "bring it back?".
            args = NewSession.arguments(name: stopped.name ?? name, directory: newDraft.directory, restore: true)
        } else {
            switch NewSession.plan(name: name, directory: newDraft.directory, sessions: sessions) {
            case .invalid(let why):
                newDraft.error = why
                return nil
            case .open(let running):
                form = nil
                return running
            case .askRestore(let stopped):
                newDraft.restoring = stopped
                return nil
            case .create(let a):
                args = a
            }
        }
        newDraft.working = true
        newDraft.error = nil
        defer { newDraft.working = false }
        do {
            try await client.newSession(args)
        } catch {
            newDraft.error = Self.describe(error)
            return nil
        }
        form = nil
        // The registry hears of it from Claude Code's first hook: wait a
        // little for it to show up, then open it.
        for _ in 0..<20 {
            await loadNow()
            if let s = sessions.first(where: { $0.name == name && $0.inTmux && $0.state != .closed && $0.state != .interrupted }) {
                return s
            }
            try? await Task.sleep(nanoseconds: 300_000_000)
        }
        say(L.t("Started \(name). It shows up here in a moment.", "已启动 \(name)，稍后就会出现在这里。"))
        return nil
    }

    func editNote(_ s: Session) {
        closeGate.cancel()
        form = .note(s)
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
            } catch {
                say(Self.describe(error))
            }
            refresh()
        }
    }

    /// First click of "Stop reminding": ask.
    func askToClose(_ list: [Session]) {
        let allowed = list.filter(Close.allowed)
        guard !allowed.isEmpty else { return }
        form = nil
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
    @ViewBuilder let content: () -> Content
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.brand)
                Text(title).font(.system(size: 14, weight: .semibold)).lineLimit(1)
            }
            .padding(.horizontal, 4)
            content()
        }
        .padding(12)
        .frame(width: Theme.panelWidth - 28)
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
            Text(text).font(.system(size: 11)).foregroundStyle(.secondary)
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
        FormCard(title: L.t("New Session", "新会话"), icon: Icons.newSession) {
            VStack(spacing: 0) {
                FormRow(title: L.t("Folder", "目录")) {
                    Dropdown(options: folderOptions, selection: draft.directory, pick: { id in
                        if id == Self.chooseID {
                            model.chooseDirectory()
                        } else {
                            model.pickDirectory(id)
                        }
                    }) {
                        HStack(spacing: 6) {
                            Image(systemName: Icons.project).font(.system(size: 11)).foregroundStyle(.secondary)
                            Text(Grouping.tilde(draft.directory, home: NSHomeDirectory()))
                                .truncationMode(.head)
                                .frame(maxWidth: 230, alignment: .leading)
                        }
                    }
                    .tip(draft.directory)
                    Spacer(minLength: 0)
                }
                FormRow(title: L.t("Name", "名字"), divider: false) {
                    FilledField(placeholder: L.t("e.g. login-page", "比如 登录页"),
                                text: Binding(get: { model.newDraft.name },
                                              set: { model.newDraft.name = $0; model.newDraft.nameEdited = true
                                                  model.newDraft.restoring = nil; model.newDraft.error = nil }),
                                autofocus: true,
                                onSubmit: submit)
                }
            }
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.primary.opacity(0.025)))
            hint
            footer
        }
    }

    static let chooseID = "\u{0}choose"

    private func folderOptions() -> [DropdownOption<String>] {
        let recent = NewSession.recentProjects(model.sessions)
        let home = NSHomeDirectory()
        var options = (recent.contains(draft.directory) || draft.directory.isEmpty ? recent : [draft.directory] + recent)
            .map { DropdownOption(id: $0, title: Grouping.tilde($0, home: home), symbol: Icons.project) }
        options.append(DropdownOption(id: Self.chooseID, title: L.t("Choose…", "选择…"), symbol: Icons.chooseFolder))
        return options
    }

    @ViewBuilder private var hint: some View {
        if let error = draft.error {
            FormHint(text: error, icon: Icons.warning, tint: Theme.amber)
        } else if let stopped = draft.restoring {
            FormHint(text: L.t("“\(stopped.title)” stopped earlier. Bring it back instead?",
                               "「\(stopped.title)」之前断了，找回它？"),
                     icon: Icons.restore, tint: Theme.red)
        } else {
            switch NewSession.hint(name: draft.name, sessions: model.sessions) {
            case .free:
                FormHint(text: L.t("Starts in tmux and opens in your terminal.", "在 tmux 里开起来，再在你的终端里打开。"))
            case .running(let s):
                FormHint(text: L.t("“\(s.title)” is already running: Return opens it.", "「\(s.title)」已经在运行了，回车直接打开它。"),
                         icon: Icons.open, tint: Theme.brand)
            case .interrupted(let s):
                FormHint(text: L.t("A stopped session is called “\(s.title)”; you'll be asked to bring it back.",
                                   "有个断了的会话也叫「\(s.title)」，确认时会问你要不要找回。"),
                         icon: Icons.restore, tint: Theme.red)
            case .invalid(let why):
                FormHint(text: why, icon: Icons.warning, tint: Theme.amber)
            }
        }
    }

    private var footer: some View {
        HStack(spacing: 8) {
            if draft.working {
                ProgressView().controlSize(.small).scaleEffect(0.7)
                Text(L.t("Starting…", "正在开…")).font(.system(size: 12)).foregroundStyle(.secondary)
            }
            Spacer()
            SoftButton(title: L.t("Cancel", "取消"), icon: "xmark") { model.form = nil }
                .tip("Esc")
            SoftButton(title: primaryTitle, icon: primaryIcon, prominent: true, action: submit)
                .tip("↵")
                .disabled(draft.working)
        }
    }

    private var primaryTitle: String {
        if draft.restoring != nil { return L.t("Bring Back", "找回") }
        if case .running = NewSession.hint(name: draft.name, sessions: model.sessions) { return L.t("Open", "打开") }
        return L.t("Create", "新建")
    }

    private var primaryIcon: String {
        if draft.restoring != nil { return Icons.restore }
        if case .running = NewSession.hint(name: draft.name, sessions: model.sessions) { return Icons.open }
        return Icons.newSession
    }

    private func submit() {
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

    var body: some View {
        FormCard(title: L.t("Note for “\(session.title)”", "给「\(session.title)」写便条"), icon: Icons.note) {
            FilledField(placeholder: L.t("Where it stopped, what's next…", "停在哪里、下一步做什么…"),
                        text: $text, autofocus: true) { model.saveNote(session, text: text) }
            FormHint(text: L.t("Return saves, Esc cancels, empty clears the note.", "回车保存，Esc 取消，留空就是清除。"))
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark") { model.form = nil }
                    .tip("Esc")
                SoftButton(title: L.t("Save", "保存"), icon: "checkmark", prominent: true) {
                    model.saveNote(session, text: text)
                }
                .tip("↵")
            }
        }
    }
}

// MARK: - Stop reminding

struct CloseConfirmCard: View {
    @Bindable var model: AppModel
    let sessions: [Session]

    var body: some View {
        FormCard(title: Close.question(sessions), icon: Icons.dismiss) {
            FormHint(text: L.t("It won't be offered for restore again. `cc-moss ls --all` still lists it.",
                               "以后不会再提示找回它；`cc-moss ls --all` 里仍然看得到。"))
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark") { model.closeGate.cancel() }
                    .tip("Esc")
                SoftButton(title: L.t("Stop Reminding", "不再提醒"), icon: Icons.dismiss, prominent: true) {
                    model.confirmClose()
                }
                .tip("↵")
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

/// The "+" beside the logo in the action bar: start a new session (⌘N).
struct NewSessionButton: View {
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: Icons.newSession)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(hovering ? .primary : .secondary)
                .frame(width: 26, height: 26)
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
