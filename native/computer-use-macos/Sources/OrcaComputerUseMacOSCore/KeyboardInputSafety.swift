public enum KeyboardInputSafety {
    public enum FocusFailure: Equatable {
        case targetNotFocused
        case targetNotFocusedAfterRestore
    }

    public static func syntheticInputFocusFailure(targetWindowFocused: Bool, restoreWindowRequested: Bool, targetApplicationFrontmost: Bool) -> FocusFailure? {
        guard !targetWindowFocused || !targetApplicationFrontmost else {
            return nil
        }
        return restoreWindowRequested ? .targetNotFocusedAfterRestore : .targetNotFocused
    }
}
