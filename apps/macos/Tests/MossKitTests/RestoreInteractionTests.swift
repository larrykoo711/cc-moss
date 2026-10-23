import Testing
@testable import MossKit

@Suite struct RestoreSourceInteractionTests {
    @Test func settingsLabelIsNotAWorkingDirectoryLabel() {
        L.$override.withValue(.zh) {
            #expect(RestoreSettingsCopy.locationLabel == "设置读取位置")
        }
    }

    @Test func disabledSuppressesFeedbackAndActivation() {
        let feedback = RestoreControlFeedback(isEnabled: false, hovered: true, pressed: true, focused: true, preview: .hover)
        #expect(feedback.state == .disabled)
        #expect(!feedback.showsFocus && !feedback.emphasized)
        var calls = 0
        let activated = feedback.activate { calls += 1 }
        #expect(!activated && calls == 0)
    }

    @Test func pressKeepsKeyboardFocusWithoutChangingGeometry() {
        let feedback = RestoreControlFeedback(isEnabled: true, hovered: true, pressed: true, focused: true)
        #expect(feedback.state == .pressed && feedback.showsFocus)
    }

    @Test(arguments: [RestoreControlState.idle, .hover, .pressed, .focused])
    func enabledStatesPerformExactlyOneAction(_ state: RestoreControlState) {
        let feedback = RestoreControlFeedback(isEnabled: true, preview: state)
        var calls = 0
        let activated = feedback.activate { calls += 1 }
        #expect(activated && calls == 1)
    }

    @Test func folderActionExplainsThatWorkingDirectoryDoesNotChange() {
        L.$override.withValue(.zh) {
            #expect(RestoreSettingsCopy.changeFolder == "更换设置文件夹")
            #expect(RestoreSettingsCopy.folderHelp.contains("不会改变会话工作目录"))
        }
    }

    @Test(arguments: [false, true])
    func unknownMeansSourceWasNotRecordedNotThatSettingsWereLost(_ advanced: Bool) {
        L.$override.withValue(.zh) {
            #expect(RestoreSettingsCopy.unknown(advanced: advanced).contains("旧会话未记录设置来源"))
        }
    }
}
