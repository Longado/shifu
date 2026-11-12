// Disposable native file selection; no network or application submission.
import AppKit

@MainActor
final class FilePickerFixture: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let result = NSTextField(labelWithString: "Selected: none")
    let count = NSTextField(labelWithString: "Selections: 0")
    var selections = 0

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(contentRect: NSRect(x: 40, y: 90, width: 920, height: 640),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Shifu File Picker Fixture"
        window.isReleasedWhenClosed = false
        let button = NSButton(title: "Choose test file", target: self, action: #selector(chooseFile))
        button.frame = NSRect(x: 30, y: 550, width: 180, height: 32)
        result.frame = NSRect(x: 30, y: 490, width: 850, height: 24)
        count.frame = NSRect(x: 30, y: 450, width: 850, height: 24)
        for view in [button, result, count] { window.contentView!.addSubview(view) }
        window.orderBack(nil)
    }

    @objc func chooseFile() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        if let directory = ProcessInfo.processInfo.environment["SHIFU_PICKER_SAMPLE_DIR"] {
            panel.directoryURL = URL(fileURLWithPath: directory, isDirectory: true)
        }
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .OK, let url = panel.url else { return }
            self.selections += 1
            self.result.stringValue = "Selected: " + url.lastPathComponent
            self.count.stringValue = "Selections: \(self.selections)"
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = FilePickerFixture()
app.delegate = delegate
app.run()
