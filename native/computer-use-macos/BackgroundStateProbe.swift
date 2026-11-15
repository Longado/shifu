// Read metadata only; never inspect clipboard contents or send input.
import AppKit
import CoreGraphics

let frontPID = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let frontWindow = windows.first {
    ($0[kCGWindowOwnerPID as String] as? Int) == Int(frontPID) &&
    ($0[kCGWindowLayer as String] as? Int) == 0
}
let mouse = NSEvent.mouseLocation
let result: [String: Any] = [
    "frontmostAppPid": Int(frontPID),
    "frontmostOnscreenWindowId": frontWindow?[kCGWindowNumber as String] as? Int ?? 0,
    "mouseX": Double(mouse.x),
    "mouseY": Double(mouse.y),
    "clipboardChangeCount": NSPasteboard.general.changeCount,
]
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
