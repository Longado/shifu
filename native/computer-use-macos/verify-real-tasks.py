#!/usr/bin/env python3
"""Opt-in macOS check: creates disposable Finder files and two local Safari pages.

Usage: python3 native/computer-use-macos/verify-real-tasks.py /tmp/shifu-cu-check-UNIQUE
Honors SHIFU_COMPUTER_HOME/HELPER; does not close existing windows or delete evidence.
"""
import http.server
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import threading
import time
import uuid

from task_pages import cleanup_pages

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=False)
cli = os.environ.get('SHIFU_CLI', 'shifu')
owned = []
owned_pages = []
page_token = uuid.uuid4().hex
results = {"tasks": [], "cleanupErrors": []}
stage = "setup"
last_error_code = None
front_bundle = None
try:
    front = subprocess.check_output(['lsappinfo', 'front'], text=True, timeout=5).strip()
    front_info = subprocess.check_output(['lsappinfo', 'info', '-only', 'bundleID', front], text=True, timeout=5)
    front_bundle = re.search(r'="([^"]+)"', front_info)
except Exception as error:
    results['cleanupErrors'].append({'stage': 'capture_front', 'message': str(error)})


def call(name, app, *args, require=True):
    global stage, last_error_code
    stage = name
    process = subprocess.run([cli, 'computer', *map(str, args), '--app', app, '--no-screenshot'], capture_output=True, text=True, timeout=45)
    if not process.stdout:
        raise RuntimeError(f'{name}: command returned no JSON (exit {process.returncode})')
    result = json.loads(process.stdout)
    last_error_code = (result.get('error') or {}).get('code')
    (root / f'{name}.json').write_text(process.stdout)
    if require:
        assert result['ok'], (name, result.get('error'))
    return result


def windows(app):
    result = call('windows-' + app, app, 'list-windows', require=False)
    if (result.get('error') or {}).get('code') == 'app_not_found':
        return []
    assert result['ok'], result.get('error')
    return result['result']['windows']


def await_window(app, title, track_window=True):
    until = time.monotonic() + 8
    while time.monotonic() < until:
        matches = [w for w in windows(app) if w['title'] == title]
        if len(matches) == 1:
            if track_window:
                owned.append((app, matches[0]['id']))
                (root / 'owned-windows.json').write_text(json.dumps([{'app': a, 'window': w} for a, w in owned], indent=2))
            return matches[0]['id']
        time.sleep(.15)
    raise RuntimeError(f'window not ready: {title}')


class Page(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        label = 'A' if self.path.endswith('/a') else 'B'
        title = f'Shifu computer check {label} {root.name}'
        html = f'<!doctype html><html><meta charset="utf-8"><title>{title}</title><body><main><h1>{title}</h1><p>Local test page {label} is ready.</p></main></body></html>'.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(html)))
        self.end_headers()
        self.wfile.write(html)

    def log_message(self, *_):
        pass


server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Page)
threading.Thread(target=server.serve_forever, daemon=True).start()
def finder_task():
    folder = root / ('Finder-' + root.name)
    folder.mkdir()
    old = folder / '改名前.txt'
    new = folder / '改名完成.txt'
    old.write_text('Shifu real Finder task\n')
    subprocess.run(['open', '-R', str(old)], check=True, timeout=10)
    main = await_window('com.apple.finder', folder.name)
    call('finder-main', 'com.apple.finder', 'get-app-state', '--window-id', main, '--restore-window')
    call('finder-enter', 'com.apple.finder', 'press-key', '--window-id', main, '--key', 'Return', '--restore-window')
    until = time.monotonic() + 3
    while True:
        editor = call('finder-editor', 'com.apple.finder', 'get-app-state')['result']['snapshot']
        if editor.get('focusedInputWindowId') == editor['window']['id'] and editor['focusedElementId'] is not None:
            break
        assert time.monotonic() < until, 'standalone rename editor was not exposed'
        time.sleep(.1)
    assert editor['window']['height'] < 48, 'regression must exercise the small editor'
    editor_id = editor['window']['id']
    call('finder-value', 'com.apple.finder', 'set-value', '--window-id', editor_id, '--element-index', editor['focusedElementId'], '--value', new.name, '--restore-window')
    committed = call('finder-commit', 'com.apple.finder', 'press-key', '--window-id', editor_id, '--key', 'Return', '--expect-path', new, '--expect-missing-path', old, '--timeout-ms', 2500)
    assert committed['result']['postcondition']['state'] == 'met'
    assert committed['result']['action']['targetWindowId'] == editor_id
    assert not old.exists() and new.read_text() == 'Shifu real Finder task\n'
    results['finder_rename'] = True



def browser_task():
    safari = 'com.apple.Safari'
    browser_ids = []
    for label in ['A', 'B']:
        url = f'http://127.0.0.1:{server.server_port}/{page_token}/{label.lower()}'
        owned_pages.append(url)
        inventory = root / 'owned-pages.json'
        temporary = inventory.with_suffix('.tmp')
        temporary.write_text(json.dumps(owned_pages, indent=2))
        temporary.replace(inventory)
        script = f'tell application "Safari"\nmake new document with properties {{URL:"{url}"}}\n'
        if label == 'A':
            script += 'activate\n'
        subprocess.run(['osascript', '-e', script + 'end tell'], check=True, capture_output=True, timeout=10)
        browser_ids.append(await_window(safari, f'Shifu computer check {label} {root.name}', track_window=False))
    a, b = browser_ids
    call('browser-a', safari, 'get-app-state', '--window-id', a, '--restore-window')
    call('browser-select-b', safari, 'get-app-state', '--window-id', b, '--restore-window')
    assert call('browser-focused-b', safari, 'get-app-state')['result']['snapshot']['window']['id'] == b
    call('browser-back-a', safari, 'get-app-state', '--window-id', a, '--restore-window')
    fallback = call('browser-fallback', safari, 'get-app-state', '--window-id', b, '--wait-for', 'INTENTIONALLY-ABSENT-PROBE', '--foreground-fallback', '--timeout-ms', 600, require=False)
    assert fallback['result']['postcondition']['foregroundFallbackUsed']
    assert call('browser-restored-a', safari, 'get-app-state')['result']['snapshot']['window']['id'] == a
    index = next(w['index'] for w in windows(safari) if w['id'] == b)
    selected = call('browser-index-b', safari, 'get-app-state', '--window-index', index, '--restore-window')
    assert selected['result']['snapshot']['window']['id'] == b
    assert call('browser-index-focused-b', safari, 'get-app-state')['result']['snapshot']['window']['id'] == b
    results['browser_target_and_restore'] = True

try:
    for task_id, task in [('finder_rename', finder_task), ('browser_target_and_restore', browser_task)]:
        started = time.monotonic()
        stage = 'setup'
        last_error_code = None
        record = {'taskId': task_id, 'status': 'not_run', 'failedStage': None,
                  'errorCode': None, 'durationMs': 0, 'evidence': str(root),
                  'humanIntervention': 'unknown', 'foregroundInterruptions': 'unknown'}
        try:
            task()
            record['status'] = 'passed'
        except Exception as error:
            results[task_id] = False
            record.update(status='failed', failedStage=stage,
                          errorCode=last_error_code or type(error).__name__, message=str(error))
        finally:
            record['durationMs'] = round((time.monotonic() - started) * 1000)
            results['tasks'].append(record)
            (root / 'result.json').write_text(json.dumps(results, ensure_ascii=False, indent=2))
finally:
    closed = []
    for app, window in reversed(owned):
        try:
            state = call('close-state-' + str(window), app, 'get-app-state', '--window-id', window, '--match', 'close button', require=False)
            tree = ((state.get('result') or {}).get('snapshot') or {}).get('treeText', '')
            match = re.search(r'^\s*(\d+) close button', tree, re.M)
            if match:
                call('close-' + str(window), app, 'click', '--window-id', window, '--element-index', match[1], require=False)
            closed.append({'app': app, 'window': window, 'closed': all(w['id'] != window for w in windows(app))})
        except Exception as error:
            closed.append({'app': app, 'window': window, 'closed': None})
            results['cleanupErrors'].append({'stage': 'close_window', 'window': window, 'message': str(error)})
    results['cleanup'] = closed
    try:
        results['pageCleanup'] = cleanup_pages(owned_pages)
        if results['pageCleanup']['remaining']:
            results['cleanupErrors'].append({'stage': 'close_pages', 'message': 'owned Safari URLs remain open'})
    except Exception as error:
        results['cleanupErrors'].append({'stage': 'close_pages', 'message': str(error)})
    server.shutdown()
    server.server_close()
    if front_bundle:
        try:
            subprocess.run(['open', '-b', front_bundle[1]], check=True, timeout=10)
        except Exception as error:
            results['cleanupErrors'].append({'stage': 'restore_front', 'message': str(error)})
    (root / 'result.json').write_text(json.dumps(results, ensure_ascii=False, indent=2))
print(json.dumps(results, ensure_ascii=False))
raise SystemExit(0 if all(t['status'] == 'passed' for t in results['tasks']) and not results['cleanupErrors'] and all(w['closed'] for w in closed) else 1)
