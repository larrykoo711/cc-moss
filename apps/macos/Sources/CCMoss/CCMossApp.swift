import AppKit
import MossKit
import SwiftUI

struct CCMossApp: App {
    /// Owns the model and the status item (`StatusItem.swift`): the menu bar
    /// icon animates by setting the button's image, outside SwiftUI.
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        // Never inserted: the status item is ours. It stays as the first
        // scene so that, as before, launching opens no window.
        MenuBarExtra(isInserted: .constant(false)) {
            EmptyView()
        } label: {
            EmptyView()
        }

        // Our own window rather than the stock Settings scene: no title bar,
        // the panel's frosted surface, Raycast-style tabs.
        Window(L.t("cc-moss Settings", "cc-moss 设置"), id: "settings") {
            SettingsView(model: delegate.model)
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .defaultPosition(.center)
    }
}

@main
enum Main {
    static func main() {
        // `--render out.png` draws the panel from demo data and exits:
        // screenshots for the README and for visual review, no menu bar needed.
        // `--preview --demo file.json`: the panel in an ordinary window, for
        // watching animations and recording them.
        if CommandLine.arguments.contains("--bench-icon") {
            Render.benchIcon(args: CommandLine.arguments)
            return
        }
        if CommandLine.arguments.contains("--preview") {
            Render.preview(args: CommandLine.arguments)
            return
        }
        if let i = CommandLine.arguments.firstIndex(of: "--render"), i + 1 < CommandLine.arguments.count {
            Render.run(out: CommandLine.arguments[i + 1], args: CommandLine.arguments)
            return
        }
        CCMossApp.main()
    }
}
