import AppKit
import MossKit
import SwiftUI

struct CCMossApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        MenuBarExtra {
            PanelView(model: model)
        } label: {
            Image(nsImage: StatusIcon.image)
                .accessibilityLabel("cc-moss")
        }
        .menuBarExtraStyle(.window)

        // Our own window rather than the stock Settings scene: no title bar,
        // the panel's frosted surface, Raycast-style tabs.
        Window(L.t("cc-moss Settings", "cc-moss 设置"), id: "settings") {
            SettingsView(model: model)
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
        // `--pet-sheets dir`: the desktop pet design prototypes (T3.1).
        if let i = CommandLine.arguments.firstIndex(of: "--pet-sheets"), i + 1 < CommandLine.arguments.count {
            PetSheets.run(outDir: CommandLine.arguments[i + 1])
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
