import XCTest
@testable import OrcaComputerUseMacOSCore

final class TreeTextDiffTests: XCTestCase {
    let baseline = "App=Fixture\nWindow: Test\n0 window\n\t1 button Count\n\t2 text Count 0\n\t3 text Note\nThe focused UI element is 1 button Count."
    func testValueChangeAndNoChangeKeepContext() {
        let after = baseline.replacingOccurrences(of: "2 text Count 0", with: "2 text Count 1")
        let diff = TreeTextDiff.render(before: baseline, after: after)
        XCTAssertTrue(diff?.contains("~ \t2 text Count 1") == true)
        XCTAssertTrue(diff?.contains("App=Fixture") == true)
        XCTAssertTrue(diff?.contains("The focused UI element") == true)
        XCTAssertFalse(diff?.contains("\n\t1 button Count\n") == true)
        XCTAssertTrue(TreeTextDiff.render(before: baseline, after: baseline)?.contains("No change") == true)
    }
    func testLeafRemovalAndStructuralChanges() {
        XCTAssertTrue(TreeTextDiff.render(before: baseline, after: baseline.replacingOccurrences(of: "\n\t3 text Note", with: ""))?.contains("- \t3 text Note") == true)
        for after in [baseline + "\n\t4 button New", baseline.replacingOccurrences(of: "\t2 text", with: "\t\t2 text"), baseline.replacingOccurrences(of: "\t1 button Count\n\t2 text Count 0", with: "\t2 text Count 0\n\t1 button Count")] {
            XCTAssertNil(TreeTextDiff.render(before: baseline, after: after))
        }
        XCTAssertNil(TreeTextDiff.render(before: baseline, after: ""))
    }
    func testContainerRemovalDuplicateAndLargeChangeUseFullTree() {
        let old = "0 window\n\t1 group\n\t\t2 text Child\n\t3 text Other"
        XCTAssertNil(TreeTextDiff.render(before: old, after: "0 window\n\t3 text Other"))
        XCTAssertNil(TreeTextDiff.render(before: old, after: old + "\n\t3 text Duplicate"))
        XCTAssertNil(TreeTextDiff.render(before: "0 window\n\t1 text A", after: "0 window changed\n\t1 text B"))
        let many = (0..<50).map { "\($0) text A" }.joined(separator: "\n")
        XCTAssertNil(TreeTextDiff.render(before: many, after: many.replacingOccurrences(of: "text A", with: "text B")))
    }

}
