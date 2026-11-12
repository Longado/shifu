import XCTest
@testable import OrcaComputerUseMacOSCore

final class ForegroundFocusTests: XCTestCase {
    func testWindowIdentityAloneCannotAuthorizeForegroundKeys() {
        XCTAssertEqual(KeyboardInputSafety.syntheticInputFocusFailure(
            targetWindowFocused: true, restoreWindowRequested: true, targetApplicationFrontmost: false
        ), .targetNotFocusedAfterRestore)
        XCTAssertEqual(KeyboardInputSafety.syntheticInputFocusFailure(
            targetWindowFocused: true, restoreWindowRequested: false, targetApplicationFrontmost: false
        ), .targetNotFocused)
        XCTAssertNil(KeyboardInputSafety.syntheticInputFocusFailure(
            targetWindowFocused: true, restoreWindowRequested: true, targetApplicationFrontmost: true
        ))
        XCTAssertEqual(KeyboardInputSafety.syntheticInputFocusFailure(
            targetWindowFocused: false, restoreWindowRequested: true, targetApplicationFrontmost: true
        ), .targetNotFocusedAfterRestore)
        XCTAssertEqual(KeyboardInputSafety.syntheticInputFocusFailure(
            targetWindowFocused: false, restoreWindowRequested: true, targetApplicationFrontmost: false
        ), .targetNotFocusedAfterRestore)
    }
}
