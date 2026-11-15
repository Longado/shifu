public enum BackgroundOnlyPolicy {
    public enum Failure: Error, CustomStringConvertible {
        case foregroundRequested
        case unsupportedMethod(String)
        case unsupportedValueTarget

        public var description: String {
            switch self {
            case .foregroundRequested:
                return "background-only mode rejects foreground restoration and fallback"
            case let .unsupportedMethod(method):
                return "background-only mode does not allow \(method); use a browser page API or an explicitly authorized foreground flow"
            case .unsupportedValueTarget:
                return "background-only setValue requires a text field or text area with a string value"
            }
        }
    }

    public static let methods = ["handshake", "listApps", "listWindows", "getAppState", "setValue"]

    public static func validate(method: String, restoreWindow: Bool = false, briefForeground: Bool = false, foregroundFallback: Bool = false) throws {
        guard !restoreWindow, !briefForeground, !foregroundFallback else {
            throw Failure.foregroundRequested
        }
        // Even AXPress may open a native dialog. The initial strict mode therefore permits no clicks.
        guard methods.contains(method) else { throw Failure.unsupportedMethod(method) }
    }

    public static func validateValueTarget(role: String?, stringValue: Bool) throws {
        guard stringValue, role == "AXTextField" || role == "AXTextArea" else {
            throw Failure.unsupportedValueTarget
        }
    }
}
