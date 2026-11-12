// A disposable target for real computer-use acceptance; it has no external effects.
import AppKit

@MainActor
final class Fixture: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let countLabel = NSTextField(labelWithString: "点击次数：0")
    var count = 0
    var otherWindow: NSWindow?
    let observationMode = ProcessInfo.processInfo.environment["SHIFU_FIXTURE_OBSERVATION"] == "1"
    let counterPath = ProcessInfo.processInfo.environment["SHIFU_FIXTURE_COUNTER_PATH"]

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
        if observationMode {
            let close = NSButton(title: "关闭测试窗口", target: self, action: #selector(closeTestWindows))
            close.frame = NSRect(x: 24, y: 58, width: 140, height: 28)
            window.contentView!.addSubview(close)
            let delayed = NSButton(title: "延迟完成", target: self, action: #selector(completeLater))
            delayed.frame = NSRect(x: 200, y: 58, width: 120, height: 28)
            window.contentView!.addSubview(delayed)
            let other = NSWindow(contentRect: NSRect(x: 600, y: 120, width: 460, height: 240),
                                 styleMask: [.titled, .closable], backing: .buffered, defer: false)
            other.title = "Shifu Computer Use 另一个测试窗口"
            other.isReleasedWhenClosed = false
            let label = NSTextField(labelWithString: "另一个窗口，不接收主窗口的操作结果。")
            label.frame = NSRect(x: 24, y: 120, width: 400, height: 28)
            other.contentView!.addSubview(label)
            otherWindow = other
            other.orderBack(nil)
            window.isReleasedWhenClosed = false
            window.orderFront(nil)
            recordCount()
        } else {
            window.orderBack(nil)
        }
    }

    @objc func increment() {
        count += 1
        countLabel.stringValue = "点击次数：\(count)"
        if observationMode {
            recordCount()
            otherWindow?.orderFront(nil)
        }
    }

    @objc func completeLater() {
        countLabel.stringValue = "正在处理"
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in
            self?.countLabel.stringValue = "异步处理完成"
        }
    }

    @objc func closeTestWindows() {
        count += 1
        recordCount()
        otherWindow?.close()
        window.close()
    }

    private func recordCount() {
        guard let counterPath else { return }
        do {
            try String(count).write(toFile: counterPath, atomically: true, encoding: .utf8)
        } catch {
            NSLog("Fixture counter write failed: %@", String(describing: error))
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = Fixture()
app.delegate = delegate
app.run()
