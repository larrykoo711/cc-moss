import AppKit
import MossKit

/// The app's appearance: Settings › General › Appearance, applied to every
/// window at once (the panel, Mo's bubbles, Settings, the welcome) through
/// the app's own appearance, which they all inherit.
@MainActor enum AppAppearance {
    static func apply(_ choice: AppearanceChoice = .load()) {
        NSApp.appearance = choice.appearanceName.map { NSAppearance(named: NSAppearance.Name($0)) } ?? nil
    }

    static func choose(_ choice: AppearanceChoice) {
        choice.save()
        apply(choice)
    }
}
