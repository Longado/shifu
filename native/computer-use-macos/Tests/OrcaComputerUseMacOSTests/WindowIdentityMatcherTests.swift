import XCTest
@testable import OrcaComputerUseMacOSCore

final class WindowIdentityMatcherTests: XCTestCase {
    func testKnownDifferentParentCannotMatchContainedPanel() {
        let panel = CGRect(x: 50, y: 50, width: 300, height: 200)
        XCTAssertNil(WindowIdentityMatcher.match([.init(id: 120, frame: CGRect(x: 0, y: 0, width: 1000, height: 800))], target: 71705, frame: panel))
        XCTAssertNil(WindowIdentityMatcher.match([.init(id: 120, frame: panel)], target: 71705, frame: panel))
        XCTAssertEqual(WindowIdentityMatcher.match([.init(id: 71705, frame: panel)], target: 71705, frame: panel), 0)
    }
    func testMissingIdentityNeedsUniqueNearEqualGeometry() {
        let panel = CGRect(x: 50, y: 50, width: 300, height: 200)
        XCTAssertEqual(WindowIdentityMatcher.match([.init(id: nil, frame: panel.offsetBy(dx: 1, dy: 1))], target: 3, frame: panel), 0)
        XCTAssertNil(WindowIdentityMatcher.match([.init(id: nil, frame: panel), .init(id: nil, frame: panel)], target: 3, frame: panel))
        XCTAssertNil(WindowIdentityMatcher.match([.init(id: nil, readable: false, frame: panel)], target: 3, frame: panel))
    }
    func testIndependentInputRootRequiresExactIdentity() {
        let editor = CGRect(x: 10, y: 10, width: 200, height: 23)
        XCTAssertEqual(WindowIdentityMatcher.match([.init(id: 42, frame: editor, allowsGeometry: false)], target: 42, frame: editor), 0)
        XCTAssertNil(WindowIdentityMatcher.match([.init(id: nil, frame: editor, allowsGeometry: false)], target: 42, frame: editor))
        XCTAssertNil(WindowIdentityMatcher.match([.init(id: 43, frame: editor, allowsGeometry: false)], target: 42, frame: editor))
    }

    func testRecoveryFindsFocusedSheetAndNeverFallsBackToParent() {
        let parent = WindowIdentityMatcher.Candidate(id: 120, frame: CGRect(x: 0, y: 0, width: 900, height: 800))
        let sheet = WindowIdentityMatcher.Candidate(id: 72416, frame: CGRect(x: 0, y: 357, width: 880, height: 448))
        XCTAssertEqual(WindowIdentityMatcher.recoveryTarget([parent, sheet], target: 72416, frame: nil), 1)
        XCTAssertNil(WindowIdentityMatcher.recoveryTarget([parent], target: 72416, frame: nil))
        XCTAssertNil(WindowIdentityMatcher.recoveryTarget([parent], target: 72416, frame: parent.frame))
        XCTAssertNil(WindowIdentityMatcher.recoveryTarget([sheet, sheet], target: nil, frame: sheet.frame))
    }

}
