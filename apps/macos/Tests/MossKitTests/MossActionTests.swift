import Foundation
import Testing
@testable import MossKit

@Suite struct MossActionTests {
    @Test func thereAreActions() {
        #expect(!MossAction.all.isEmpty)
    }

    @Test func idsAreUnique() {
        #expect(Set(MossAction.all.map(\.id)).count == MossAction.all.count)
    }

    @Test func bothLanguagesArePresent() {
        for action in MossAction.all {
            for lang in [Lang.en, .zh] {
                #expect(!action.title(lang).isEmpty, "\(action.id)")
                #expect(!action.detail(lang).isEmpty, "\(action.id)")
                #expect(!action.text(lang).isEmpty, "\(action.id)")
            }
            #expect(action.title(.en) != action.title(.zh), "\(action.id)")
            #expect(action.text(.en) != action.text(.zh), "\(action.id)")
        }
    }

    @Test func todaysAbilities() {
        #expect(MossAction.all.map(\.id) == ["checkup", "needs-you", "stuck", "today", "tidy"])
        #expect(MossAction.all[1].text(.zh) == "现在哪些会话在等我？按急的先说。")
    }

    @Test func checkupAndTidySayExactlyThis() throws {
        let checkup = try #require(MossAction.all.first { $0.id == "checkup" })
        #expect(checkup.title(.zh) == "体检" && checkup.title(.en) == "Checkup" && checkup.symbol == "stethoscope")
        #expect(checkup.text(.zh) == "体检一下：能修的直接修，修完给我一段简短的体检简报；要我定的给我选项。")
        #expect(checkup.text(.en) == "Give my sessions a checkup: fix what you can, then a short brief; give me options for what I need to decide.")
        #expect(checkup.detail(.zh) == "能修的直接修，要你定的给你选项")
        let tidy = try #require(MossAction.all.first { $0.id == "tidy" })
        #expect(tidy.title(.zh) == "整理会话" && tidy.title(.en) == "Tidy up")
        #expect(tidy.text(.zh) == "整理一下会话：没名字的补名字，断了很久的和闲着很久的给我选项。")
        #expect(tidy.text(.en) == "Tidy up my sessions: name the unnamed ones, and give me options for long-dropped or long-idle ones.")
        #expect(tidy.detail(.zh) == "补名字、清理很久没动的")
    }

    @Test func dailySwitchesAreOnUnlessSaidOtherwise() {
        #expect(MossDaily.isOn(nil), "an older cc-moss without the key")
        #expect(MossDaily.isOn("true") && MossDaily.isOn(" TRUE ") && MossDaily.isOn("garbled"))
        #expect(!MossDaily.isOn("false") && !MossDaily.isOn("0") && !MossDaily.isOn("off"))
        #expect(MossDaily.value(true) == "true" && MossDaily.value(false) == "false")
        #expect(MossDaily.checkupKey == "moss.daily_checkup" && MossDaily.briefKey == "moss.daily_brief")
    }

    @MainActor @Test func tappingSendsTheActionsText() async {
        let runner = AskRunner([(["moss", "say"], ok(#"{"schema":1,"id":"1760608900000-abcdef"}"#))])
        let client = MossClient(executable: "/x/cc-moss", runner: runner)
        let desk = MossDesk { try await client.mossSay($0) }
        let action = MossAction.all[1]
        await desk.perform(action, lang: .en)
        #expect(runner.calls == [["moss", "say", "--json", "--", action.text(.en)]])
        #expect(desk.messages.map(\.text) == [action.text(.en)], "the words actually sent")
        #expect(desk.messages.first?.delivery == .queued)
    }
}
