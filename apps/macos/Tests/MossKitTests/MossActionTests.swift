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
        #expect(MossAction.all.map(\.id) == ["needs-you", "stuck", "today"])
        #expect(MossAction.all[0].text(.zh) == "现在哪些会话在等我？按急的先说。")
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
