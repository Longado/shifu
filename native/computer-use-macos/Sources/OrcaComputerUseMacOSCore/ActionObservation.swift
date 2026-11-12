import Foundation

/// Parameters for observing the window that actually received input.
public enum ActionObservation {
    public static func parameters<Value>(_ params: [String: Value], targetWindowId: Value, restoreWindow: Value) -> [String: Value] {
        var result = params
        result.removeValue(forKey: "windowIndex")
        result["windowId"] = targetWindowId
        result["restoreWindow"] = restoreWindow
        return result
    }
}

/// A completed input must remain distinguishable from an input that never ran.
public struct ActionObservationFailure: Error, @unchecked Sendable {
    public let action: [String: Any]
    public let causeCode: String
    public let causeMessage: String

    public init(action: [String: Any], causeCode: String, causeMessage: String) {
        self.action = action
        self.causeCode = causeCode
        self.causeMessage = causeMessage
    }

    public func response(requestId: Int) -> [String: Any] {
        [
            "id": requestId,
            "ok": false,
            "error": [
                "code": "observation_failed",
                "message": "Input was performed, but its result could not be observed. Do not repeat the action without checking the app.",
            ],
            "result": [
                "action": action,
                "observationError": ["code": causeCode, "message": causeMessage],
            ],
        ]
    }
}
