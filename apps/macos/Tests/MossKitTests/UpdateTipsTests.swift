import Foundation
import Testing
@testable import MossKit

private func info(current: String = "0.3.1", latest: String, available: Bool = true) -> UpdateInfo {
    try! UpdateInfo.decode(Data(#"{"schema":1,"current":"\#(current)","latest":"\#(latest)","available":\#(available)}"#.utf8))
}

private let entry = ChangelogEntry(
    version: "0.3.2", date: "2026-10-21",
    headline: .init(zh: "标题", en: "Headline"),
    highlights: ["一", "二", "三", "四"].enumerated().map { i, zh in .init(zh: zh, en: "h\(i + 1)") },
    short: .init(zh: "短", en: "Short"))

@Suite struct UpdateTipsVersionTests {
    @Test func comparesNumbersNotText() {
        #expect(UpdateTips.isNewer("0.3.10", than: "0.3.9"))
        #expect(!UpdateTips.isNewer("0.3.9", than: "0.3.10"))
        #expect(UpdateTips.isNewer("1.0.0", than: "0.99.99"))
        #expect(!UpdateTips.isNewer("0.3.2", than: "0.3.2"))
    }

    @Test func aReleaseIsNewerThanItsPreRelease() {
        #expect(UpdateTips.isNewer("0.4.0", than: "0.4.0-rc.1"))
        #expect(UpdateTips.isNewer("0.4.0-rc.2", than: "0.4.0-rc.1"))
    }

    @Test func somethingThatIsNotAVersionIsNeverNewer() {
        #expect(!UpdateTips.isNewer("dev", than: "0.3.1"))
        #expect(!UpdateTips.isNewer("0.3.2", than: "dev"))
    }

    @Test func anchorFollowsTheSite() {
        #expect(UpdateTips.anchor("0.3.1") == "v0-3-1")
        #expect(UpdateTips.anchor("0.3.10") == "v0-3-10")
    }

    @Test func changelogLinkIsInYourLanguage() {
        #expect(UpdateTips.changelogURL("0.3.2", lang: .en).absoluteString == "https://cc-moss.larrykoo.com/changelog/#v0-3-2")
        #expect(UpdateTips.changelogURL("0.3.2", lang: .zh).absoluteString == "https://cc-moss.larrykoo.com/zh/changelog/#v0-3-2")
        #expect(UpdateTips.changelogURL(nil, lang: .en).absoluteString == "https://cc-moss.larrykoo.com/changelog/")
    }
}

@Suite struct UpdateTipsBarTests {
    @Test func showsTheNewerVersion() {
        #expect(UpdateTips.bar(app: "0.3.1", info: info(latest: "0.3.2"), checksOn: true, dismissed: nil) == "0.3.2")
    }

    @Test func closedForThatVersionOnly() {
        #expect(UpdateTips.bar(app: "0.3.1", info: info(latest: "0.3.2"), checksOn: true, dismissed: "0.3.2") == nil)
        #expect(UpdateTips.bar(app: "0.3.1", info: info(latest: "0.3.3"), checksOn: true, dismissed: "0.3.2") == "0.3.3")
    }

    @Test func goneOnceTheAppRunsIt() {
        #expect(UpdateTips.bar(app: "0.3.2", info: info(current: "0.3.2", latest: "0.3.2"), checksOn: true, dismissed: nil) == nil)
        // The command updated first and still says available: the app is what counts.
        #expect(UpdateTips.bar(app: "0.3.2", info: info(latest: "0.3.2"), checksOn: true, dismissed: nil) == nil)
    }

    @Test func neverWhenChecksAreOff() {
        #expect(UpdateTips.bar(app: "0.3.1", info: info(latest: "0.3.2"), checksOn: false, dismissed: nil) == nil)
    }

    @Test func nothingWithoutACheckOrWhenNoneIsOut() {
        #expect(UpdateTips.bar(app: "0.3.1", info: nil, checksOn: true, dismissed: nil) == nil)
        #expect(UpdateTips.bar(app: "0.3.1", info: info(latest: "0.3.1", available: false), checksOn: true, dismissed: nil) == nil)
    }

    @Test func aDevBuildGoesByTheCommandsVersion() {
        #expect(UpdateTips.bar(app: "dev", info: info(latest: "0.3.2"), checksOn: true, dismissed: nil) == "0.3.2")
    }
}

@Suite struct UpdateTipsBubbleTests {
    @Test func oncePerVersion() {
        #expect(UpdateTips.bubble(app: "0.3.1", info: info(latest: "0.3.2"), checksOn: true, bubbled: nil) == "0.3.2")
        #expect(UpdateTips.bubble(app: "0.3.1", info: info(latest: "0.3.2"), checksOn: true, bubbled: "0.3.2") == nil)
        #expect(UpdateTips.bubble(app: "0.3.1", info: info(latest: "0.3.3"), checksOn: true, bubbled: "0.3.2") == "0.3.3")
    }

    @Test func neverWhenChecksAreOff() {
        #expect(UpdateTips.bubble(app: "0.3.1", info: info(latest: "0.3.2"), checksOn: false, bubbled: nil) == nil)
    }

    @Test func followsWhereAlertsGo() {
        #expect(UpdateTips.moTells(in: AlertContext(petEnabled: true, petVisible: true)))
        #expect(!UpdateTips.moTells(in: AlertContext(petEnabled: false)), "Mo is off")
        #expect(!UpdateTips.moTells(in: AlertContext(petEnabled: true, petVisible: false)), "Mo is away")
        #expect(!UpdateTips.moTells(in: AlertContext(petEnabled: true, petVisible: true, doNotDisturb: true)))
        #expect(!UpdateTips.moTells(in: AlertContext(petEnabled: true, petVisible: true, panelOpen: true)))
        #expect(!UpdateTips.moTells(in: AlertContext(mode: .notificationsOnly, petEnabled: true, petVisible: true)))
    }

    @Test func moSaysTheVersion() {
        L.$override.withValue(.zh) {
            let words = PetMessage(UpdateTips.alert(version: "0.3.2", id: 1, at: Date())).text(names: ["0.3.2"])
            #expect(words.title == "cc-moss 0.3.2 来了")
            #expect(words.primary == "更新")
            #expect(words.secondary == "稍后")
        }
        L.$override.withValue(.en) {
            let words = PetMessage(UpdateTips.alert(version: "0.3.2", id: 1, at: Date())).text(names: ["0.3.2"])
            #expect(words.title == "cc-moss 0.3.2 is out")
            #expect(!words.title.contains("!") && !words.body.contains("!"))
        }
    }

    @Test func neverOutLoud() {
        #expect(!VoicePolicy.sounds(.update, in: .active))
        #expect(!AlertKind.update.defaultOn)
    }
}

@Suite struct UpdateTipsWhatsNewTests {
    private let log = Changelog(versions: [entry])

    @Test func freshInstallShowsNothingAndRemembers() {
        let r = UpdateTips.whatsNew(app: "0.3.2", lastSeen: nil, changelog: log)
        #expect(r.show == nil)
        #expect(r.remember == "0.3.2")
    }

    @Test func afterAnUpgradeShowsTheEntryOnce() {
        let r = UpdateTips.whatsNew(app: "0.3.2", lastSeen: "0.3.1", changelog: log)
        #expect(r.show == entry)
        #expect(r.remember == "0.3.2")
        let again = UpdateTips.whatsNew(app: "0.3.2", lastSeen: "0.3.2", changelog: log)
        #expect(again.show == nil)
        #expect(again.remember == nil)
    }

    @Test func noEntryNoCard() {
        let r = UpdateTips.whatsNew(app: "0.3.3", lastSeen: "0.3.2", changelog: log)
        #expect(r.show == nil)
        #expect(r.remember == "0.3.3")
        #expect(UpdateTips.whatsNew(app: "0.3.2", lastSeen: "0.3.1", changelog: nil).show == nil)
    }

    @Test func aDevBuildIsLeftAlone() {
        let r = UpdateTips.whatsNew(app: "dev", lastSeen: "0.3.1", changelog: log)
        #expect(r.show == nil)
        #expect(r.remember == nil)
    }

    @Test func theCardWinsOverTheBar() {
        #expect(UpdateTips.panelTip(whatsNew: entry, bar: "0.3.3") == .whatsNew(entry))
        #expect(UpdateTips.panelTip(whatsNew: nil, bar: "0.3.3") == .update(version: "0.3.3"))
        #expect(UpdateTips.panelTip(whatsNew: nil, bar: nil) == PanelTip.none)
    }

    @Test func atMostThreeHighlightsInYourLanguage() {
        #expect(UpdateTips.highlights(entry, lang: .en) == ["h1", "h2", "h3"])
        #expect(UpdateTips.highlights(entry, lang: .zh) == ["一", "二", "三"])
    }
}

@Suite struct ChangelogTests {
    @Test func decodesAndLooksUp() throws {
        let json = #"""
        {"$comment": "x", "versions": [
          {"version": "0.3.2", "date": "2026-10-21", "icon": "moss",
           "short": {"zh": "短", "en": "Short"},
           "headline": {"zh": "标题", "en": "Headline"},
           "highlights": [{"zh": "一", "en": "One"}]},
          {"version": "0.3.1", "date": "2026-10-20",
           "headline": {"zh": "旧", "en": "Old"}, "highlights": []}
        ]}
        """#
        let log = try Changelog.decode(Data(json.utf8))
        #expect(log.versions.count == 2)
        #expect(log.entry(for: "0.3.2")?.headline.text(.en) == "Headline")
        #expect(log.entry(for: "0.3.1")?.short == nil)
        #expect(log.entry(for: "0.9.9") == nil)
    }

    @Test func aMissingOrBrokenFileIsNil() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("changelog-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        #expect(Changelog.load(dir.appendingPathComponent("none.json")) == nil)
        let bad = dir.appendingPathComponent("bad.json")
        try Data("{".utf8).write(to: bad)
        #expect(Changelog.load(bad) == nil)
        #expect(Changelog.load(nil) == nil)
    }

    @Test func theEnvironmentPointsAtAFile() {
        let url = Changelog.location(environment: [Changelog.environmentKey: "/tmp/x/changelog.json"])
        #expect(url?.path == "/tmp/x/changelog.json")
    }

    /// The file bundle.sh ships decodes, and every entry has what the card needs.
    @Test func theSitesFileDecodes() throws {
        let file = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("../../site/src/changelog.json").standardizedFileURL
        let log = try Changelog.decode(Data(contentsOf: file))
        #expect(!log.versions.isEmpty)
        for v in log.versions {
            #expect(VersionMatch.parse(v.version) != nil, "\(v.version)")
            #expect(!v.headline.zh.isEmpty && !v.headline.en.isEmpty)
        }
    }
}

@Suite struct UpdateTipsMemoryTests {
    @Test func remembersEachVersionApart() {
        let defaults = UserDefaults(suiteName: "UpdateTipsMemoryTests-\(UUID().uuidString)")!
        let memory = UpdateTipsMemory(defaults: defaults)
        #expect(memory.lastSeen == nil && memory.dismissed == nil && memory.bubbled == nil)
        memory.lastSeen = "0.3.1"
        memory.dismissed = "0.3.2"
        memory.bubbled = "0.3.3"
        let again = UpdateTipsMemory(defaults: defaults)
        #expect(again.lastSeen == "0.3.1")
        #expect(again.dismissed == "0.3.2")
        #expect(again.bubbled == "0.3.3")
    }
}

@Suite struct PetDirectorUpdateTests {
    private func session(_ id: Int, _ kind: AlertKind = .needsYou) -> SessionAlert {
        SessionAlert(id: id, slotId: "A", kind: kind, name: "job", at: Date(timeIntervalSince1970: 0))
    }

    private var news: SessionAlert { UpdateTips.alert(version: "0.3.2", id: -1, at: Date(timeIntervalSince1970: 0)) }

    @Test func comesWhenMoIsFree() {
        var d = PetDirector(mode: .active, phrases: Catchphrases(seed: 1))
        let r1 = d.take(news, at: 0)
        #expect(r1)
        #expect(d.message?.kind == .update)
    }

    @Test func waitsBehindAQuestion() {
        var d = PetDirector(mode: .active, phrases: Catchphrases(seed: 1))
        d.take(session(1), at: 0)
        let r2 = d.take(news, at: 1)
        #expect(!r2, "never over it")
        #expect(d.message?.kind == .needsYou)
        #expect(d.waiting?.kind == .update)
        let r3 = d.takeQueued(at: 2)
        #expect(!r3, "still busy")
        d.dismiss()
        let r4 = d.takeQueued(at: 3)
        #expect(r4)
        #expect(d.message?.kind == .update)
        #expect(d.waiting == nil)
    }

    @Test func pushedAsideByAQuestionAndBackAfter() {
        var d = PetDirector(mode: .active, phrases: Catchphrases(seed: 1))
        d.take(news, at: 0)
        let r5 = d.take(session(1), at: 1)
        #expect(r5)
        #expect(d.message?.kind == .needsYou)
        #expect(d.waiting?.kind == .update)
        d.dismiss()
        let r6 = d.takeQueued(at: 2)
        #expect(r6)
        #expect(d.message?.kind == .update)
    }

    @Test func evenADoneTurnGoesFirst() {
        var d = PetDirector(mode: .active, phrases: Catchphrases(seed: 1))
        d.take(session(1, .finished), at: 0)
        d.take(news, at: 1)
        #expect(d.message?.kind == .finished)
    }

    @Test func restingKeepsItWaiting() {
        var d = PetDirector(mode: .active, phrases: Catchphrases(seed: 1))
        d.take(session(1), at: 0)
        d.take(news, at: 1)
        d.dismiss()
        d.doNotDisturb = true
        let r7 = d.takeQueued(at: 2)
        #expect(!r7)
        #expect(d.waiting != nil)
        d.doNotDisturb = false
        let r8 = d.takeQueued(at: 3)
        #expect(r8)
    }
}
