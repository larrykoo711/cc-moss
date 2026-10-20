import Foundation
import Testing
@testable import MossKit

@Suite struct FailureNoticeTests {
    let subject = FailureNotice.Subject(name: "fix-invoice", id: "01c0c1d2e-0000-4000", folder: "/Users/demo/billing")

    private func restore(_ json: String) throws -> RestoreResult {
        try RestoreReport.decode(Data(#"{"schema":1,"results":[\#(json)]}"#.utf8)).results[0]
    }

    @Test func theSubjectKeepsEightCharactersOfTheId() {
        #expect(subject.id == "01c0c1d2")
    }

    @Test func titleSaysWhatDidntWorkInChinese() {
        L.$override.withValue(.zh) {
            #expect(FailureNotice(action: .restore, subject: subject).title == "没能找回「fix-invoice」")
        }
    }

    @Test func titleSaysWhatDidntWorkInEnglish() {
        L.$override.withValue(.en) {
            #expect(FailureNotice(action: .restart, subject: subject).title == "Couldn't restart “fix-invoice”")
        }
    }

    @Test func eachRestoreReasonHasItsOwnWhy() throws {
        try L.$override.withValue(.en) {
            let codes = ["empty", "missing_dir", "exited_at_once", "tmux_failed", "already_running", "ambiguous"]
            let whys = try codes.map { code in
                FailureNotice(restore: try restore(#"{"id":"01C","name":"fix-invoice","ok":false,"reason":"\#(code)","message":"m"}"#),
                              subject: subject).why
            }
            #expect(Set(whys).count == codes.count)
            #expect(!whys.contains { $0.hasPrefix("cc-moss said") })
        }
    }

    @Test func missingFolderSaysWhatToTry() throws {
        L.$override.withValue(.zh) {
            let n = FailureNotice(action: .restore, subject: subject, reason: "missing_dir")
            #expect(n.why == "它的文件夹不在了。把文件夹放回去，或者不再提醒它。")
        }
    }

    @Test func ambiguousListsTheCandidatesAsNames() throws {
        let r = try restore(#"{"id":"01C","name":"fix","ok":false,"reason":"ambiguous","candidates":["fix-a","fix-b"]}"#)
        #expect(r.reason == .ambiguous)
        L.$override.withValue(.en) {
            #expect(FailureNotice(restore: r, subject: nil).why == "More than one session matches: fix-a, fix-b. Ask Moss to sort them out.")
        }
    }

    @Test func ambiguousCandidatesCanBeObjects() throws {
        let r = try restore(#"{"id":"01C","name":"fix","ok":false,"reason":"ambiguous","candidates":[{"name":"fix","id":"0123456789"},{"name":"fix"}]}"#)
        #expect(r.candidates == ["fix (01234567)", "fix"])
    }

    @Test func aNewerReasonCodeIsKeptAsWritten() throws {
        let r = try restore(#"{"id":"01C","name":"x","ok":false,"reason":"brand_new","message":"it broke"}"#)
        #expect(r.reason == .other && r.reasonCode == "brand_new")
        L.$override.withValue(.en) {
            #expect(FailureNotice(restore: r, subject: nil).why == "cc-moss said: it broke")
        }
    }

    @Test func refusalsFromBatchResultsUseTheirReason() {
        L.$override.withValue(.en) {
            let n = FailureNotice(action: .restart, batch: BatchResult(name: "x", outcome: "refused", reason: "not-in-tmux"), subject: nil)
            #expect(n.why == "It isn't in tmux. Move it into tmux first.")
            #expect(n.subject?.name == "x")
        }
    }

    @Test func anErrorBecomesItsMessage() {
        let n = FailureNotice(action: .send, subject: subject, error: MossClient.Failure.command("no such session"))
        #expect(n.reason == nil && n.message == "no such session")
        let t = FailureNotice(action: .send, subject: subject, error: MossClient.Failure.timedOut)
        #expect(t.reason == "timeout")
    }

    @Test func nothingSaidStillGivesAWay() {
        L.$override.withValue(.en) {
            #expect(FailureNotice(action: .archive, subject: subject).why == "cc-moss didn't say why. Ask Moss to look into it.")
        }
    }

    @Test func theMessageToMossInChinese() {
        L.$override.withValue(.zh) {
            let n = FailureNotice(action: .restore, subject: subject, reason: "missing_dir", message: "目录不存在")
            #expect(n.mossText == "「找回」会话 fix-invoice（01c0c1d2，/Users/demo/billing）失败了：missing_dir：目录不存在。帮我看看怎么解决。")
        }
    }

    @Test func theMessageToMossInEnglish() {
        L.$override.withValue(.en) {
            let n = FailureNotice(action: .restore, subject: subject, reason: "tmux_failed", message: "no server")
            #expect(n.mossText == "“Bring back” failed for session fix-invoice (01c0c1d2, /Users/demo/billing): tmux_failed: no server. Please help me fix it.")
        }
    }

    @Test func theMessageToMossCarriesCandidates() {
        L.$override.withValue(.en) {
            let n = FailureNotice(action: .restore, subject: .init(name: "fix"), reason: "ambiguous", candidates: ["fix-a", "fix-b"])
            #expect(n.mossText == "“Bring back” failed for session fix: ambiguous: candidates: fix-a, fix-b. Please help me fix it.")
        }
    }

    @Test func theMessageToMossWithoutASessionOrReason() {
        L.$override.withValue(.zh) {
            #expect(FailureNotice(action: .newSession, subject: nil).mossText == "「新建会话」失败了：没有给出原因。帮我看看怎么解决。")
        }
    }

    @Test func detailsHoldEveryFact() {
        L.$override.withValue(.en) {
            let d = FailureNotice(action: .restore, subject: subject, reason: "empty", message: "nothing said").details
            for part in ["fix-invoice", "01c0c1d2", "/Users/demo/billing", "empty", "nothing said", "restore"] {
                #expect(d.contains(part), "\(part)")
            }
        }
    }

    @Test func theNewestShowsWithACountBehindIt() {
        var list = FailureNotices()
        list.add(FailureNotice(id: "a", action: .restore, subject: subject))
        list.add(FailureNotice(id: "b", action: .archive, subject: subject))
        #expect(list.current?.id == "b" && list.more == 1)
        list.dismiss()
        #expect(list.current?.id == "a" && list.more == 0)
        list.dismiss("a")
        #expect(list.current == nil)
    }
}
