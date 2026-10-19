import Foundation
import Testing
@testable import MossKit

@Suite struct PanelLinkTests {
    private func link(_ s: String) -> PanelLink? { URL(string: s).flatMap(PanelLink.init) }

    @Test func pages() {
        #expect(link("cc-moss://panel") == .panel)
        #expect(link("cc-moss://panel/") == .panel)
        #expect(link("cc-moss://panel/moss") == .moss)
        #expect(link("cc-moss://panel/queue") == .queue)
        #expect(link("cc-moss://panel/todo") == .queue)
        #expect(link("CC-MOSS://Panel/Sessions") == .sessions)
    }

    @Test func notOurs() {
        #expect(link("cc-moss://panel/settings") == nil)
        #expect(link("cc-moss://other") == nil)
        #expect(link("https://panel/moss") == nil)
    }
}
