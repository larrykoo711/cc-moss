import MossKit
import SwiftUI

/// Settings › Mo: on or off, how big, and how it
/// gets your attention (DESIGN-pet §6.5).
struct PetSettings: View {
    @AppStorage(PetController.enabledKey) private var enabled = false
    @AppStorage(PetController.sizeKey) private var size = Double(PetController.defaultSize)
    @AppStorage(PetInteraction.key) private var interaction = PetInteraction.everyday.rawValue
    @AppStorage(VoiceMode.key) private var voiceMode = VoiceMode.off.rawValue
    /// Empty: the best installed voice (Automatic).
    @AppStorage(VoicePicker.key) private var voiceId = ""

    @Environment(\.colorScheme) private var scheme

    var body: some View {
        MoStudio(enabled: $enabled, size: $size, changed: changed)
        if enabled {
            SettingsCard(title: L.t("When Something Needs You", "有事找你时")) {
                CardRow(title: L.t("How chatty", "互动方式"), detail: detail, divider: false) {
                    SlidingPicker(selection: $interaction, options: [
                        (PetInteraction.quiet.rawValue, L.t("Quiet", "安静")),
                        (PetInteraction.everyday.rawValue, L.t("Everyday", "日常")),
                        (PetInteraction.active.rawValue, L.t("Active", "积极")),
                    ])
                    .onChange(of: interaction) { _, _ in changed() }
                }
            }
            .transition(.opacity)
            voiceCard
                .transition(.opacity)
        }
    }

    // MARK: - Voice (T3.4)

    private var mode: VoiceMode { VoiceMode(rawValue: voiceMode) ?? .off }

    /// Off, a chime, or Mo reading the message title (DESIGN-pet §6.4).
    private var voiceCard: some View {
        SettingsCard(title: L.t("Voice", "声音")) {
            CardRow(title: L.t("Sound", "出声方式"), detail: voiceDetail, divider: mode != .off) {
                SlidingPicker(selection: $voiceMode.animation(.easeOut(duration: 0.2)), options: [
                    (VoiceMode.off.rawValue, L.t("Off", "关")),
                    (VoiceMode.chime.rawValue, L.t("Chime", "提示音")),
                    (VoiceMode.speak.rawValue, L.t("Speak", "说话")),
                ])
            }
            switch mode {
            case .off:
                EmptyView()
            case .chime:
                CardRow(title: L.t("Chime", "提示音"), detail: L.t("A soft sound built into macOS.", "macOS 自带的一声轻响。"),
                        divider: false) {
                    SoftButton(title: L.t("Preview", "试听"), icon: "play.fill") { Speaker.shared.chime() }
                }
                .transition(.opacity)
            case .speak:
                voiceRows
                    .transition(.opacity)
            }
        }
    }

    @ViewBuilder private var voiceRows: some View {
        let voices = VoicePicker.installed(Speaker.installedVoices(), for: L.lang)
        let chosen = voices.first { $0.id == voiceId } ?? voices.first
        CardRow(title: L.t("Voice", "嗓音"), detail: voiceCaption(voices)) {
            HStack(spacing: 8) {
                if !voices.isEmpty {
                    Dropdown(options: { voiceOptions(voices) }, selection: voices.contains { $0.id == voiceId } ? voiceId : "",
                             pick: { voiceId = $0 }) {
                        Text(voices.contains { $0.id == voiceId } ? chosen.map(title) ?? "" : L.t("Automatic", "自动"))
                    }
                }
                SoftButton(title: L.t("Preview", "试听"), icon: "play.fill") {
                    Speaker.shared.say(L.t("Hi, I'm Moss. This is how I'll call you when something needs you.",
                                           "你好，我是小莫。有事找你时，我就这样说。"),
                                       voice: chosen?.id, language: VoicePicker.languageCode(L.lang))
                }
            }
        }
        LinkRow(icon: "arrow.down.circle", title: L.t("Download Better Voices", "下载更好的声音"),
                url: Speaker.downloadVoicesURL, divider: false)
    }

    private var voiceDetail: String {
        switch mode {
        case .off: L.t("Mo stays silent and only makes faces.", "小莫不出声，只用表情示意。")
        case .chime: L.t("A soft chime, at most every 30 s.", "有事时轻响一声，30 秒最多一次。")
        case .speak: L.t("Mo reads the title aloud, at most every 30 s.", "小莫念出消息标题，30 秒最多一句。")
        }
    }

    private func voiceCaption(_ voices: [VoiceInfo]) -> String {
        if voices.isEmpty {
            return L.t("No English voice yet. Download one in System Settings.", "还没有中文声音，先去系统设置下载一个。")
        }
        return voices.contains { $0.quality > .standard }
            ? L.t("Premium and Enhanced voices sound the most natural.", "「高级」「增强」声音听起来最自然。")
            : L.t("Downloaded voices sound far more natural.", "下载一个「高级」声音，听起来自然得多。")
    }

    /// Automatic first (the best one, named), then every installed voice, best first.
    private func voiceOptions(_ voices: [VoiceInfo]) -> [DropdownOption<String>] {
        [DropdownOption(id: "", title: L.t("Automatic", "自动"), symbol: "wand.and.stars", badge: voices.first.map(title))]
            + voices.map { DropdownOption(id: $0.id, title: title($0), badge: $0.quality.tag) }
    }

    /// The voice's name, with its region when it is not the usual one (Meijia · Taiwan).
    private func title(_ voice: VoiceInfo) -> String {
        let parts = voice.language.replacingOccurrences(of: "_", with: "-").split(separator: "-")
        guard voice.language.replacingOccurrences(of: "_", with: "-") != VoicePicker.languageCode(L.lang),
              parts.count > 1,
              let region = Locale(identifier: L.lang == .zh ? "zh_CN" : "en_US").localizedString(forRegionCode: String(parts[1]))
        else { return voice.name }
        return "\(voice.name) · \(region)"
    }

    /// What the chosen mode does when something needs you.
    private var detail: String {
        switch PetInteraction(rawValue: interaction) ?? .everyday {
        case .quiet:
            L.t("Mo only makes a face. Click it to see what's up.",
                "小莫只用表情和动作示意，点它才说是什么事。")
        case .everyday:
            L.t("Mo makes a face and murmurs a word or two. Hover over it to read more.",
                "小莫用表情示意，再嘀咕一两个字。鼠标移过去就展开说。")
        case .active:
            L.t("Mo pops up the message right away, buttons and all.",
                "小莫直接弹出消息框，按钮一并给你。")
        }
    }

    private func changed() {
        NotificationCenter.default.post(name: PetController.settingsChanged, object: nil)
    }
}
