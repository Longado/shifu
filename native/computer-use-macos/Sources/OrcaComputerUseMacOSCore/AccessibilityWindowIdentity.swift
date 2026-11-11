import ApplicationServices
import CoreGraphics
import Darwin
import Foundation

public enum AccessibilityWindowIdentity {
    private enum Failure: Error {
        case unreadable
    }

    /// Finder exposes its inline rename editor as a focused AXTextField directly in
    /// AXWindows, with a real WindowServer ID but no AXFocusedWindow/AXWindow parent.
    public static func focusedInputWindowID(of app: AXUIElement) -> CGWindowID? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute as CFString, &value) == .success,
              let value, CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
        let input = value as! AXUIElement
        var appPID: pid_t = 0, inputPID: pid_t = 0
        guard AXUIElementGetPid(app, &appPID) == .success,
              AXUIElementGetPid(input, &inputPID) == .success, appPID == inputPID else { return nil }
        var role: CFTypeRef?, focused: CFTypeRef?, windows: CFTypeRef?
        guard AXUIElementCopyAttributeValue(input, kAXRoleAttribute as CFString, &role) == .success,
              ["AXTextField", "AXTextArea", "AXComboBox"].contains(role as? String ?? ""),
              AXUIElementCopyAttributeValue(input, kAXFocusedAttribute as CFString, &focused) == .success,
              focused as? Bool == true,
              AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows) == .success,
              let windows = windows as? [AXUIElement], windows.contains(where: { CFEqual($0, input) }) else { return nil }
        return try? windowID(of: input)
    }

    /// Nil permits geometry fallback; unreadable identity must refuse input.
    public static func windowID(of element: AXUIElement) throws -> CGWindowID? {
        var nativeAbsent = true
        if let getWindow {
            var windowID: CGWindowID = 0
            let status = getWindow(element, &windowID)
            if status == .success, windowID != 0 { return windowID }
            nativeAbsent = status == .attributeUnsupported || status == .noValue
        }

        var value: CFTypeRef?
        switch AXUIElementCopyAttributeValue(element, "AXWindowNumber" as CFString, &value) {
        case .success:
            guard let number = value as? NSNumber,
                  CFGetTypeID(number) != CFBooleanGetTypeID(),
                  let windowID = CGWindowID(exactly: number.doubleValue), windowID != 0
            else { throw Failure.unreadable }
            return windowID
        case .attributeUnsupported, .noValue:
            guard nativeAbsent else { throw Failure.unreadable }
            return nil
        default:
            throw Failure.unreadable
        }
    }

    private typealias GetWindow = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError

    // AppKit and WebKit windows often omit AXWindowNumber.
    private static let getWindow: GetWindow? = {
        guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "_AXUIElementGetWindow") else { return nil }
        // SAFETY: HIServices exports AXError _AXUIElementGetWindow(AXUIElementRef, CGWindowID *).
        return unsafeBitCast(symbol, to: GetWindow.self)
    }()
}
