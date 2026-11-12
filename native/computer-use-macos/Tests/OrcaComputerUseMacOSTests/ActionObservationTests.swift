import XCTest
@testable import OrcaComputerUseMacOSCore

final class ActionObservationTests: XCTestCase {
    func testObservationPinsActualWindowAndDropsMutableIndex() {
        let result = ActionObservation.parameters(["windowIndex": 1, "windowId": 2, "restoreWindow": 1], targetWindowId: 42, restoreWindow: 0)
        XCTAssertNil(result["windowIndex"])
        XCTAssertEqual(result["windowId"], 42)
        XCTAssertEqual(result["restoreWindow"], 0)
    }

    func testObservationFailurePreservesPerformedActionInFailedEnvelope() throws {
        let failure = ActionObservationFailure(action: ["targetWindowId": 42, "action": "AXPress"], causeCode: "window_not_found", causeMessage: "closed")
        let response = failure.response(requestId: 7)
        XCTAssertEqual(response["ok"] as? Bool, false)
        XCTAssertEqual(response["id"] as? Int, 7)
        let error = try XCTUnwrap(response["error"] as? [String: Any])
        XCTAssertEqual(error["code"] as? String, "observation_failed")
        let result = try XCTUnwrap(response["result"] as? [String: Any])
        let action = try XCTUnwrap(result["action"] as? [String: Any])
        XCTAssertEqual(action["targetWindowId"] as? Int, 42)
        XCTAssertEqual(action["action"] as? String, "AXPress")
        XCTAssertNil(result["snapshot"])
        let observation = try XCTUnwrap(result["observationError"] as? [String: Any])
        XCTAssertEqual(observation["code"] as? String, "window_not_found")
    }
}
