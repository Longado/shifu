// A disposable target for real computer-use acceptance; it has no external effects.
import AppKit

@MainActor
final class Fixture: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let countLabel = NSTextField(labelWithString: "点击次数：0")
    var count = 0

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(contentRect: NSRect(x: 80, y: 120, width: 460, height: 240),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Shifu Computer Use 测试窗口"
        let button = NSButton(title: "增加计数", target: self, action: #selector(increment))
        button.frame = NSRect(x: 24, y: 155, width: 120, height: 32)
        countLabel.frame = NSRect(x: 170, y: 160, width: 220, height: 24)
        let field = NSTextField(string: "初始文字")
        field.setAccessibilityLabel("测试输入框")
        field.frame = NSRect(x: 24, y: 90, width: 380, height: 28)
        let note = NSTextField(labelWithString: "仅测试窗口，不读取或修改其他文件。")
        note.frame = NSRect(x: 24, y: 32, width: 410, height: 24)
        for view in [button, countLabel, field, note] { window.contentView!.addSubview(view) }
        window.orderBack(nil)
    }

    @objc func increment() {
        count += 1
        countLabel.stringValue = "点击次数：\(count)"
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = Fixture()
app.delegate = delegate
app.run()
