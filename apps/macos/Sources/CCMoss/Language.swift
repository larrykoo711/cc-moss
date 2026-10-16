import MossKit
import Observation
import SwiftUI

/// The interface language, shared with the CLI's `[ui] language` (A10):
/// read once at launch, written through `cc-moss config set`. Windows that
/// follow it are rebuilt when it changes, so a switch shows at once.
@MainActor @Observable
final class LanguageState {
    static let shared = LanguageState()

    private(set) var lang: Lang = L.lang
    private(set) var choice: LangChoice = .auto

    /// What the config file says, as the CLI reads it.
    func load(from client: MossClient) async {
        guard let value = try? await client.config("ui.language") else { return }
        apply(LangChoice(config: value))
    }

    /// From Settings: save it for the CLI too, then switch.
    func choose(_ new: LangChoice, client: MossClient?) async throws {
        try await client?.setConfig("ui.language", new.configValue)
        apply(new)
    }

    private func apply(_ new: LangChoice) {
        choice = new
        let next = Lang.detect(config: new.configValue)
        guard next != lang else { return }
        L.lang = next
        lang = next
        onChange?()
    }

    /// The app's other half (the CLI client's language, the menu bar label).
    @ObservationIgnored var onChange: (() -> Void)?
}

/// Rebuilds the view when the interface language changes.
private struct FollowsLanguage: ViewModifier {
    func body(content: Content) -> some View {
        content.id(LanguageState.shared.lang)
    }
}

extension View {
    func followsLanguage() -> some View {
        modifier(FollowsLanguage())
    }
}
