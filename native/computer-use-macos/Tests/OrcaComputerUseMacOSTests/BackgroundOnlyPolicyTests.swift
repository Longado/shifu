import XCTest
@testable import OrcaComputerUseMacOSCore

final class BackgroundOnlyPolicyTests: XCTestCase {
    func testRejectsActionsThatMayUseKeysClipboardOrDialogs() {
        for method in ["click", "performSecondaryAction", "typeText", "pasteText", "pressKey", "hotkey", "scroll", "drag"] {
            XCTAssertThrowsError(try BackgroundOnlyPolicy.validate(method: method), method)
        }
        for method in ["getAppState", "listApps", "listWindows", "setValue"] {
            XCTAssertNoThrow(try BackgroundOnlyPolicy.validate(method: method), method)
        }
    }

    func testRejectsBothForegroundReadRoutesAndRestoration() {
        XCTAssertThrowsError(try BackgroundOnlyPolicy.validate(method: "getAppState", restoreWindow: true))
        XCTAssertThrowsError(try BackgroundOnlyPolicy.validate(method: "getAppState", briefForeground: true))
        XCTAssertThrowsError(try BackgroundOnlyPolicy.validate(method: "getAppState", foregroundFallback: true))
    }

    func testOnlyTextStringAssignmentsAreAllowed() {
        for role in ["AXTextField", "AXTextArea"] {
            XCTAssertNoThrow(try BackgroundOnlyPolicy.validateValueTarget(role: role, stringValue: true))
        }
        let otherRoles: [String?] = [nil, "AXCheckBox", "AXPopUpButton", "AXTabGroup", "AXWindow"]
        for role in otherRoles {
            XCTAssertThrowsError(try BackgroundOnlyPolicy.validateValueTarget(role: role, stringValue: true))
        }
        XCTAssertThrowsError(try BackgroundOnlyPolicy.validateValueTarget(role: "AXTextField", stringValue: false))
    }
}
