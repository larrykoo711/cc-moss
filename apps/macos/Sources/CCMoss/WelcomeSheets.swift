import AppKit
import MossKit
import SwiftUI

/// `CCMoss --welcome-sheets dir`: every step of the welcome, in both
/// languages and both appearances, off screen. Nothing is asked of the CLI
/// or the system (`live: false`); the terminal tools step shows a canned plan.
enum WelcomeSheets {
    @MainActor static func run(outDir: String) {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)
        let model = AppModel(client: MossClient(executable: "/demo/cc-moss", runner: CannedRunner(stdout: Data())))
        let plan = InstallPlan(schema: 1, parts: [
            .init(part: "hooks", state: .changes, summary: nil),
            .init(part: "tmux", state: .ready, summary: nil),
            .init(part: "starship", state: .ready, summary: nil),
            .init(part: "shell", state: .changes, summary: nil),
        ])
        for (lang, dark) in [(Lang.zh, false), (Lang.zh, true), (Lang.en, false), (Lang.en, true)] {
            L.lang = lang
            let tag = "\(lang == .zh ? "zh" : "en")-\(dark ? "dark" : "light")"
            for step in WelcomeFlow.Step.allCases {
                let view = WelcomeView(model: model, start: step, plan: plan, live: false)
                    .background(Theme.surface(dark ? .dark : .light).opacity(1))
                PetSheets.snapshot(view, dark: dark, to: (outDir as NSString).appendingPathComponent("welcome-\(step.rawValue + 1)-\(tag).png"))
            }
            // The shortcut step once you've tried it: keys up, the panel in miniature.
            let tried = WelcomeStage(step: .shortcut, dark: dark, hits: 1, still: 2.0)
                .frame(width: 380, height: 496)
                .background(Theme.surface(dark ? .dark : .light).opacity(1))
            PetSheets.snapshot(tried, dark: dark, to: (outDir as NSString).appendingPathComponent("welcome-4-tried-\(tag).png"))
        }
        exit(0)
    }
}
