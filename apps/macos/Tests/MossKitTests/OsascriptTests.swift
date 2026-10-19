import Foundation
import Testing
@testable import MossKit

@Suite struct OsascriptTests {
    @Test func aScriptIsReadFromStandardInput() {
        #expect(Osascript.arguments == ["-"])
    }

    @Test func automationRefusedIsTold() {
        let stderr = "37:85: execution error: Not authorized to send Apple events to Terminal. (-1743)\n"
        #expect(Osascript.result(status: 1, stderr: stderr) == .denied)
    }

    @Test func anotherErrorKeepsItsWords() {
        let stderr = "12:40: execution error: Terminal got an error: Can't get window 3. (-1728)\n"
        #expect(Osascript.result(status: 1, stderr: stderr)
            == .failed("Terminal got an error: Can't get window 3. (-1728)"))
    }

    @Test func successIsOpened() {
        #expect(Osascript.result(status: 0, stderr: "") == .opened)
    }

    @Test func aFailureWithoutWordsSaysAppleScript() {
        #expect(Osascript.result(status: 1, stderr: "") == .failed("AppleScript"))
    }
}
