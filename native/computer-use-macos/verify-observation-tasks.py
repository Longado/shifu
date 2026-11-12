#!/usr/bin/env python3
"""Exercise the real CLI and native helper against disposable AppKit windows.

Usage: verify-observation-tasks.py NEW_RESULT_DIR COMPILED_FIXTURE
Set SHIFU_CLI, SHIFU_COMPUTER_HOME and SHIFU_COMPUTER_HELPER to isolated candidates.
Only the fixture's PID is targeted. No existing app windows are changed.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=False)
cli = os.environ.get('SHIFU_CLI', 'shifu')
computer_home = Path(os.environ['SHIFU_COMPUTER_HOME']).resolve()
assert computer_home != Path.home() / '.shifu/computer', 'use an isolated broker'
counter = root / 'counter.txt'
front = subprocess.check_output(['lsappinfo', 'front'], text=True, timeout=5).strip()
front_info = subprocess.check_output(['lsappinfo', 'info', '-only', 'bundleID', front], text=True, timeout=5)
front_bundle = re.search(r'="([^"]+)"', front_info)
env = dict(os.environ, SHIFU_FIXTURE_OBSERVATION='1', SHIFU_FIXTURE_COUNTER_PATH=str(counter))
fixture_log = (root / 'fixture.log').open('w')
fixture = subprocess.Popen([str(Path(sys.argv[2]).resolve())], env=env, stdout=fixture_log, stderr=fixture_log)
app = f'pid:{fixture.pid}'
results = {}


def call(name, *args, require=True):
    p = subprocess.run([cli, 'computer', *map(str, args), '--app', app, '--no-screenshot'], capture_output=True, text=True, timeout=60)
    (root / f'{name}.stderr').write_text(p.stderr)
    (root / f'{name}.json').write_text(p.stdout)
    if not p.stdout:
        raise RuntimeError(f'{name}: no JSON (exit {p.returncode}): {p.stderr.strip()}')
    r = json.loads(p.stdout)
    if require:
        assert r['ok'], (name, r.get('error'))
    return r


def button(snapshot, text, role='button'):
    match = re.search(r'^\s*(\d+) ' + re.escape(role) + (r' ' + re.escape(text) if text else '') + r'\s*$', snapshot['treeText'], re.M)
    assert match, (text, snapshot['treeText'])
    return int(match[1])


try:
    until = time.monotonic() + 10
    while True:
        listed = call('windows-before', 'list-windows', require=False)
        windows = listed.get('result', {}).get('windows', [])
        if len(windows) == 2:
            break
        assert time.monotonic() < until, listed
        time.sleep(.15)
    main = next(w for w in windows if w['title'] == 'Shifu Computer Use 测试窗口')
    other = next(w for w in windows if w['id'] != main['id'])
    assert main['index'] < other['index'], 'fixture must start with main window first'
    focused = call('foreground-main', 'get-app-state', '--window-id', main['id'], '--restore-window')['result']['snapshot']
    assert focused['foreground']['state'] in ('target_app', 'other_app', 'unknown')
    assert focused['foreground']['targetAppPid'] == fixture.pid
    key = call('foreground-key', 'press-key', '--window-id', main['id'], '--key', 'Right', '--restore-window', require=False)
    if key['ok']:
        assert key['result']['action']['targetWindowId'] == main['id']
        results['foreground_key_guard_allows_target'] = True
    else:
        assert key['error']['code'] == 'window_not_focused', key
        results['foreground_key_guard_refuses_unconfirmed_focus'] = True
    if front_bundle:
        subprocess.run(['open', '-b', front_bundle[1]], check=True, timeout=10)
        until = time.monotonic() + 3
        while True:
            background = call('background-view', 'get-app-state', '--window-id', main['id'])['result']['snapshot']['foreground']
            if background['state'] == 'other_app':
                break
            assert time.monotonic() < until, 'original foreground app was not restored'
            time.sleep(.1)
        assert background['targetAppPid'] == fixture.pid
        assert background['frontmostAppPid'] != fixture.pid
        results['foreground_snapshot_reports_other_app'] = True
    state = call('main-before', 'get-app-state', '--window-index', main['index'])['result']['snapshot']
    assert state['window']['id'] == main['id']
    clicked = call('reordered-action', 'click', '--window-index', main['index'], '--element-index', button(state, '增加计数'),
                   '--wait-for', '点击次数：1', '--wait-for-gone', '点击次数：0', '--timeout-ms', 2000)
    assert clicked['result']['action']['targetWindowId'] == main['id']
    assert clicked['result']['snapshot']['window']['id'] == main['id']
    assert clicked['result']['postcondition']['state'] == 'met'
    assert counter.read_text().strip() == '1', 'click must execute once'
    reordered = call('windows-after', 'list-windows')['result']['windows']
    assert next(w for w in reordered if w['id'] == other['id'])['index'] < next(w for w in reordered if w['id'] == main['id'])['index']
    results['window_reorder_keeps_actual_target'] = True
    results['text_disappearance_and_appearance'] = True

    state = call('delayed-state', 'get-app-state', '--window-id', main['id'])['result']['snapshot']
    call('delayed-action', 'click', '--window-id', main['id'], '--element-index', button(state, '延迟完成'))
    delayed = call('delayed-wait', 'get-app-state', '--window-id', main['id'], '--wait-for', '异步处理完成',
                   '--foreground-fallback', '--timeout-ms', 3000)
    assert delayed['result']['postcondition']['state'] == 'met'
    assert delayed['result']['postcondition']['foregroundFallbackUsed'] is True
    assert counter.read_text().strip() == '1'
    results['async_result_after_foreground_fallback'] = True

    state = call('main-close-state', 'get-app-state', '--window-id', main['id'])['result']['snapshot']
    changed = call('closed-target-text', 'click', '--window-id', main['id'], '--element-index', button(state, '', 'close button'),
                   '--wait-for', '另一个窗口，不接收主窗口的操作结果。', '--timeout-ms', 2000, require=False)
    assert changed['ok'] is False and changed['error']['code'] == 'observation_failed', changed
    assert changed['result']['action']['targetWindowId'] == main['id']
    assert changed['result']['snapshot']['window']['id'] == other['id']
    assert changed['result']['postcondition']['state'] == 'observation_failed'
    assert counter.read_text().strip() == '1'
    results['explicit_window_text_rejects_fallback'] = True

    state = call('other-close-state', 'get-app-state', '--window-id', other['id'])['result']['snapshot']
    closed = call('closed-action', 'click', '--window-id', other['id'], '--element-index', button(state, '', 'close button'), require=False)
    assert closed['ok'] is False and closed['error']['code'] == 'observation_failed', closed
    assert closed['result']['action']['targetWindowId'] == other['id']
    assert closed['result']['action']['taskCompletion'] == 'not_checked'
    assert closed['result']['observationError']['code'] and closed['result']['observationError']['message']
    assert counter.read_text().strip() == '1', 'closing windows must not repeat the earlier action'
    assert not call('windows-closed', 'list-windows')['result']['windows']
    results['closed_window_preserves_action'] = True
finally:
    fixture.terminate()
    try:
        fixture.wait(timeout=5)
    except subprocess.TimeoutExpired:
        fixture.kill()
        fixture.wait()
    fixture_log.close()
    stop = subprocess.run([cli, 'computer', 'stop'], capture_output=True, text=True, timeout=60)
    (root / 'stop.json').write_text(stop.stdout)
    results['fixture_stopped'] = fixture.poll() is not None
    if front_bundle:
        subprocess.run(['open', '-b', front_bundle[1]], check=True, timeout=10)
    (root / 'result.json').write_text(json.dumps(results, ensure_ascii=False, indent=2))
print(json.dumps(results, ensure_ascii=False))
