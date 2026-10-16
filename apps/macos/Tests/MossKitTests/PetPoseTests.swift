import Foundation
import Testing
@testable import MossKit

@Suite struct PetPoseTests {
    private let all = sessions([
        sessionJSON(id: "A", status: "interrupted"),
        sessionJSON(id: "B", busy: true),
        sessionJSON(id: "C"),
        sessionJSON(id: "D", busy: true, attention: "permission"),
    ])

    @Test func sleepWinsOverEverything() {
        #expect(PetPose.resting(all, doNotDisturb: true) == .sleeping)
    }

    @Test func needsYouBeatsInterrupted() {
        #expect(PetPose.resting(all, doNotDisturb: false) == .needsYou)
    }

    @Test func interruptedBeatsWorking() {
        let list = sessions([sessionJSON(id: "A", busy: true), sessionJSON(id: "B", status: "interrupted")])
        #expect(PetPose.resting(list, doNotDisturb: false) == .interrupted)
    }

    @Test func workingBeatsIdle() {
        let list = sessions([sessionJSON(id: "A"), sessionJSON(id: "B", busy: true)])
        #expect(PetPose.resting(list, doNotDisturb: false) == .working)
    }

    @Test func waitingAndNothingAreIdle() {
        #expect(PetPose.resting(sessions([sessionJSON(id: "A")]), doNotDisturb: false) == .idle)
        #expect(PetPose.resting([], doNotDisturb: false) == .idle)
    }

    @Test func happyPlaysOnceThenFallsBack() {
        var stage = PetStage()
        stage.rest(.working)
        let started = stage.celebrate(at: 100)
        #expect(started)
        #expect(stage.pose(at: 100) == .happy)
        #expect(stage.pose(at: 100 + PetStage.happyDuration - 0.01) == .happy)
        #expect(stage.pose(at: 100 + PetStage.happyDuration) == .working)
        #expect(stage.performanceEnds == nil || stage.performanceEnds! <= 100 + PetStage.happyDuration)
    }

    @Test func happyFallsBackToWhatIsTrueNow() {
        var stage = PetStage()
        stage.rest(.working)
        _ = stage.celebrate(at: 100)
        stage.rest(.needsYou) // something changed during the show
        #expect(stage.pose(at: 101) == .happy)
        #expect(stage.pose(at: 103) == .needsYou)
    }

    @Test func noShowWhileAsleep() {
        var stage = PetStage()
        stage.rest(.sleeping)
        let started = stage.celebrate(at: 100)
        #expect(!started)
        #expect(stage.pose(at: 100) == .sleeping)
    }

    @Test func fallingAsleepCutsTheShowShort() {
        var stage = PetStage()
        stage.rest(.idle)
        _ = stage.celebrate(at: 100)
        stage.rest(.sleeping)
        #expect(stage.pose(at: 100.5) == .sleeping)
    }
}

@Suite struct PetPresenceTests {
    @Test func translucentAtRest() {
        let p = PetPresence()
        #expect(p.opacity(at: 0) == PetPresence.rest)
    }

    @Test func fullWhileItWantsYou() {
        var p = PetPresence()
        p.update(pose: .needsYou, hovering: false, at: 10)
        #expect(p.opacity(at: 10) == 1)
        #expect(p.opacity(at: 500) == 1)
    }

    @Test func fullWhileHovered() {
        var p = PetPresence()
        p.update(pose: .idle, hovering: true, at: 10)
        #expect(p.opacity(at: 11) == 1)
    }

    @Test func fadesBackTwoSecondsAfterThePointerLeaves() {
        var p = PetPresence()
        p.update(pose: .idle, hovering: true, at: 10)
        p.update(pose: .idle, hovering: false, at: 20)
        #expect(p.opacity(at: 21.9) == 1)
        #expect(p.fadeBackAt == 20 + PetPresence.linger)
        #expect(p.opacity(at: 22) == PetPresence.rest)
    }

    @Test func fadesBackAfterTheMatterIsHandled() {
        var p = PetPresence()
        p.update(pose: .interrupted, hovering: false, at: 10)
        p.update(pose: .working, hovering: false, at: 30)
        #expect(p.opacity(at: 31) == 1)
        #expect(p.opacity(at: 32) == PetPresence.rest)
    }

    @Test func happyAndInterruptedWantYouToo() {
        #expect(PetPresence.wantsYou(.happy))
        #expect(PetPresence.wantsYou(.interrupted))
        #expect(!PetPresence.wantsYou(.working))
        #expect(!PetPresence.wantsYou(.sleeping))
    }
}

@Suite struct PetLoopTests {
    @Test func idleBreathesAndBlinksThenGoesStill() {
        let loops = PetLoops.running(pose: .idle, since: 0, now: 10, reduceMotion: false, visible: true)
        #expect(loops == [.breathe, .blink])
        #expect(PetLoops.running(pose: .idle, since: 0, now: PetLoops.stillAfter, reduceMotion: false, visible: true).isEmpty)
        #expect(PetLoops.stillAt(pose: .idle, since: 5) == 5 + PetLoops.stillAfter)
    }

    @Test func workingFlowsForAsLongAsItWorks() {
        #expect(PetLoops.running(pose: .working, since: 0, now: 3600, reduceMotion: false, visible: true) == [.flow])
        #expect(PetLoops.stillAt(pose: .working, since: 0) == nil)
    }

    @Test func nothingLoopsWhenHiddenOrReduced() {
        #expect(PetLoops.running(pose: .working, since: 0, now: 1, reduceMotion: true, visible: true).isEmpty)
        #expect(PetLoops.running(pose: .working, since: 0, now: 1, reduceMotion: false, visible: false).isEmpty)
    }

    @Test func oneShotPosesHoldStill() {
        for pose in [PetPose.needsYou, .interrupted, .happy] {
            #expect(PetLoops.running(pose: pose, since: 0, now: 1, reduceMotion: false, visible: true).isEmpty)
        }
    }

    @Test func loopsStayAtOrBelowThirtyFrames() {
        for loop in PetLoop.allCases {
            #expect(loop.maxFramesPerSecond <= 30)
        }
    }
}

@Suite struct PetEventTests {
    @Test func decodesThePage() throws {
        let json = #"{"schema":1,"events":[{"seq":7,"slot_id":"A","name":"api","kind":"restored","detail":{},"at":"2026-10-16T12:00:00Z"}],"last_seq":7}"#
        let page = try EventPage.decode(Data(json.utf8))
        #expect(page.lastSeq == 7)
        #expect(page.events.map(\.kind) == ["restored"])
        #expect(page.events.first?.name == "api")
    }

    @Test func emptyPageKeepsTheCursor() throws {
        let page = try EventPage.decode(Data(#"{"schema":1,"events":[],"last_seq":42}"#.utf8))
        #expect(page.events.isEmpty)
        #expect(page.lastSeq == 42)
    }

    @Test func newerSchemaIsRefused() {
        #expect(throws: SnapshotError.newerSchema(2)) {
            try EventPage.decode(Data(#"{"schema":2,"events":[],"last_seq":0}"#.utf8))
        }
    }
}
