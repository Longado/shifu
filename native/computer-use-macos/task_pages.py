"""Close only the local Safari tabs created by a scripted task run."""
import json
import subprocess


def cleanup_pages(urls):
    if not urls:
        return {'closed': [], 'remaining': []}
    script = '''var safari = Application("Safari");
var urls = %s;
var closed = [];
var windows = safari.windows();
var targets = [];
for (var w = windows.length - 1; w >= 0; w--) {
    if (!windows[w]) continue;
    var tabs = windows[w].tabs() || [];
    for (var t = tabs.length - 1; t >= 0; t--) {
        var url = tabs[t].url();
        if (urls.indexOf(url) >= 0) targets.push({window: windows[w].id(), tab: t, url: url});
    }
}
// Window collection indices change when the last tab closes; resolve by ID.
targets.forEach(function(target) {
    var tab = safari.windows.byId(target.window).tabs[target.tab];
    if (tab.url() === target.url) { tab.close(); closed.push(target.url); }
});
var remaining = [];
safari.windows().forEach(function(w) {
    // Closing the last tab can leave a null window in this Apple event reply.
    if (!w) return;
    (w.tabs() || []).forEach(function(t) {
        var url = t.url();
        if (urls.indexOf(url) >= 0) remaining.push(url);
    });
});
JSON.stringify({closed: closed, remaining: remaining});''' % json.dumps(urls)
    process = subprocess.run(['osascript', '-l', 'JavaScript', '-e', script],
                             capture_output=True, text=True, timeout=15)
    if process.returncode:
        raise RuntimeError('Safari page cleanup: ' + process.stderr.strip())
    return json.loads(process.stdout)
