import Foundation
import Testing
@testable import MossKit

@Suite struct StateTests {
    // The same rules as the picker's State::of (crates/cc-moss/src/pick/app.rs).
    @Test func statesFollowThePicker() {
        let list = sessions([
            sessionJSON(id: "A", status: "running", busy: true),
            sessionJSON(id: "B", status: "running", busy: false),
            sessionJSON(id: "C", status: "interrupted"),
            sessionJSON(id: "D", status: "closed"),
        ])
        #expect(list.map(\.state) == [.working, .waiting, .interrupted, .closed])
    }

    @Test func unnamedSessionsFallBackLikeThePicker() {
        let inTmux = sessions([sessionJSON(id: "A", name: nil)])[0]
        #expect(inTmux.title == "x")
        let outside = sessions([sessionJSON(id: "B", name: nil, tmux: false)])[0]
        #expect(outside.title == "B-sid-00")
        #expect(!outside.inTmux)
    }

    @Test func noteWinsOverLastPrompt() {
        let withNote = sessions([sessionJSON(note: "stopped at the invoice script", lastPrompt: "run it")])[0]
        #expect(withNote.subtitle == "stopped at the invoice script")
        #expect(withNote.subtitleIsNote)
        let withoutNote = sessions([sessionJSON(lastPrompt: "run it")])[0]
        #expect(withoutNote.subtitle == "run it")
        #expect(!withoutNote.subtitleIsNote)
    }
}
