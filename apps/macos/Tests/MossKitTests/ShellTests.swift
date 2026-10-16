import Foundation
import Testing
@testable import MossKit

@Suite struct ShellTests {
    @Test(arguments: [
        ("job-a", "job-a"),
        ("my job", "'my job'"),
        ("it's", "'it'\\''s'"),
        ("登录页", "'登录页'"),
        ("", "''"),
        ("$(rm -rf ~)", "'$(rm -rf ~)'"),
    ])
    func quotes(_ input: String, _ expected: String) {
        #expect(Shell.quote(input) == expected)
    }

    @Test func handlePrefersName() {
        #expect(sessions([sessionJSON(id: "01A", name: "job-a")])[0].handle == "job-a")
        #expect(sessions([sessionJSON(id: "01B", name: nil)])[0].handle == "01B")
    }
}
