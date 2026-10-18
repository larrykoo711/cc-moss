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
        let workspace = UserDefaults.standard.string(forKey: NewSession.workspaceKey)
        let dir = NewSession.defaultDirectory(sessions, workspace: workspace) ?? NSHomeDirectory()
        newDraft = NewDraft(directory: dir, name: NewSession.defaultName(for: dir))
        closeGate.cancel()
        form = .newSession
    }

    func pickDirectory(_ dir: String) {
        newDraft.directory = dir
        newDraft.error = nil
        if !newDraft.nameEdited { newDraft.name = NewSession.defaultName(for: dir) }
    }

    /// "Another folder…": the system folder picker over the panel, starting
    /// at the current folder; the panel and the form stay as they were.
    func chooseDirectory(from window: NSWindow?) {
        FilePicker.choose(.folder, from: window, startingAt: newDraft.directory,
                          message: L.t("Where should the new session work?", "新会话在哪个目录里干活？"),
                          prompt: L.t("Choose", "选择")) { [weak self] path in self?.pickDirectory(path) }
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

    func writeMessage(_ s: Session) {
        closeGate.cancel()
        form = .message(s)
    }

    func startRename(_ s: Session) {
        closeGate.cancel()
        form = .rename(s)
    }

    /// Type `text` into the session, as if in its terminal.
    func sendMessage(_ s: Session, text: String, done: String) {
        guard let client else { return }
        form = nil
        Task {
            do {
                try await client.send(target: s.target, text: text)
                say(done)
            } catch {
                say(Self.describe(error))
            }
            refresh()
        }
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
    /// A quiet fact about the form, at the right of the title (where a
    /// note goes, how a message arrives) instead of a sentence under it.
    var aside: (icon: String, text: String)?
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
        FormCard(title: L.t("New Session", "新会话"), icon: Icons.newSession) {
            VStack(spacing: 0) {
                FormRow(title: L.t("Folder", "目录")) {
                    Dropdown(options: folderOptions, selection: draft.directory, pick: { id in
                        if id == Self.chooseID {
                            model.chooseDirectory(from: NSApp.keyWindow)
                        } else {
                            model.pickDirectory(id)
                        }
                    }, listWidth: 380, placeholder: L.t("Search, or type a path…", "搜索，或输入路径…"),
                       typed: { FolderChoice.typedOption($0) }) {
                        let label = FolderChoice.label(draft.directory, home: NSHomeDirectory())
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
        let workspace = UserDefaults.standard.string(forKey: NewSession.workspaceKey).flatMap { $0.isEmpty ? nil : $0 }
        // Your workspace first, then the folders sessions worked in.
        var folders = NewSession.recentProjects(model.sessions)
        if let workspace { folders = [workspace] + folders.filter { $0 != workspace } }
        if !draft.directory.isEmpty, !folders.contains(draft.directory) { folders.insert(draft.directory, at: 0) }
        var options = folders.map { dir in
            dir == workspace
                ? FolderChoice.option(dir, symbol: "star", badge: L.t("Workspace", "工作区"))
                : FolderChoice.option(dir, symbol: Icons.project)
        }
        options.append(DropdownOption(id: Self.chooseID, title: L.t("Another folder…", "其他文件夹…"), symbol: Icons.chooseFolder))
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
            SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.form = nil }
            SoftButton(title: primaryTitle, icon: primaryIcon, prominent: true, shortcut: "↵", action: submit)
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

    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        FormCard(title: L.t("Message to “\(session.title)”", "给「\(session.title)」发消息"), icon: Icons.message,
                 aside: ("terminal", L.t("Typed into it", "像在终端里打字"))) {
            MultilineField(placeholder: L.t("What should Claude do next?", "想让 Claude 接下来做什么？"), text: $text) { send() }
            HStack(spacing: 8) {
                Spacer()
                SoftButton(title: L.t("Cancel", "取消"), icon: "xmark", shortcut: "esc") { model.form = nil }
                SoftButton(title: L.t("Send", "发送"), icon: Icons.message, prominent: true, shortcut: "⌘↩") { send() }
                    .disabled(empty)
            }
        }
    }

    private func send() {
        guard !empty else { return }
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
        guard check == .ok else { return }
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
                .strokeBorder(focused ? Theme.brand.opacity(0.7) : Theme.hairline(scheme), lineWidth: focused ? 1.5 : 1)
        )
        .animation(.easeOut(duration: 0.12), value: focused)
        .onAppear { focused = true }
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
