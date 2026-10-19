import Foundation
import Testing
@testable import MossKit

@Suite struct OptionLabelTests {
    @Test func theEnglishMarkBecomesATag() {
        #expect(OptionLabel.split("Use SQLite (Recommended)") == ("Use SQLite", true))
        #expect(OptionLabel.split("Use SQLite (recommended)") == ("Use SQLite", true))
    }

    @Test func theChineseMarkBecomesATag() {
        #expect(OptionLabel.split("放在路径里（推荐）") == ("放在路径里", true))
        #expect(OptionLabel.split("放在路径里 (推荐)") == ("放在路径里", true))
    }

    @Test func otherLabelsStayAsTheyAre() {
        #expect(OptionLabel.split("Recommended reading") == ("Recommended reading", false))
        #expect(OptionLabel.split("In the path") == ("In the path", false))
    }

    @Test func aLabelThatIsOnlyTheMarkIsLeftAlone() {
        #expect(OptionLabel.split("(Recommended)") == ("(Recommended)", false))
    }
}
