#!/usr/bin/env python3
"""Select one disposable file through a native NSOpenPanel; never submit it.

Usage: verify-file-picker.py NEW_OUTPUT COMPILED_FILE_PICKER_FIXTURE
Requires an isolated SHIFU_COMPUTER_HOME; honors SHIFU_CLI/SHIFU_COMPUTER_HELPER.
All operations target only the fixture PID. Polls repeat observations, not actions.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def main():
    root = Path(sys.argv[1]).resolve()
    root.mkdir(parents=True, exist_ok=False)
    fixture_binary = Path(sys.argv[2]).resolve()
    home = Path(os.environ['SHIFU_COMPUTER_HOME']).resolve()
    if home == Path.home() / '.shifu/computer':
        raise ValueError('use an isolated SHIFU_COMPUTER_HOME')
    cli = os.environ.get('SHIFU_CLI', 'shifu')
    sample_dir = root / 'sample files'
    sample_dir.mkdir()
    sample = sample_dir / '文件 选择 test.txt'
    sample.write_text('Disposable file picker acceptance\n')
    env = dict(os.environ, SHIFU_PICKER_SAMPLE_DIR=str(sample_dir))
    report = {'status': 'failed', 'stage': 'launch', 'sample': str(sample), 'actionCount': 0,
              'humanIntervention': 'unknown', 'foregroundInterruptions': 'unknown'}
    calls = 0
    stage = 'launch'
    fixture = None
    started = time.monotonic()
    log = (root / 'fixture.log').open('w')

    def call(name, method, *args, text=None, require=True):
        nonlocal calls, stage
        stage = name
        calls += 1
        path = root / f'{calls:03d}-{name}'
        command = [cli, 'computer', method, *map(str, args), '--app', f'pid:{fixture.pid}', '--json']
        if method != 'list-windows':
            command += ['--no-screenshot']
        if method not in ('list-windows', 'get-app-state'):
            report['actionCount'] += 1
        result = subprocess.run(command, input=text, capture_output=True, text=True, timeout=45)
        path.with_suffix('.stdout').write_text(result.stdout)
        path.with_suffix('.stderr').write_text(result.stderr)
        if not result.stdout:
            raise RuntimeError(f'{name}: no JSON, exit {result.returncode}')
        value = json.loads(result.stdout)
        path.with_suffix('.json').write_text(json.dumps(value, ensure_ascii=False, indent=2))
        if require and (result.returncode != 0 or value.get('ok') is not True):
            raise RuntimeError(f'{name}: {value.get("error")}')
        return value

    def windows(name):
        value = call(name, 'list-windows', require=False)
        if value.get('error', {}).get('code') == 'app_not_found':
            return []
        if value.get('ok') is not True:
            raise RuntimeError(f'{name}: {value.get("error")}')
        return value['result']['windows']

    def observe(name, window):
        return call(name, 'get-app-state', '--window-id', window)['result']['snapshot']

    def wait(name, probe, timeout=8):
        nonlocal stage
        deadline = time.monotonic() + timeout
        while True:
            stage = name
            value = probe()
            if value is not None:
                return value
            if time.monotonic() >= deadline:
                stage = name
                raise TimeoutError(name)
            time.sleep(.15)

    def control(snapshot, role, label):
        match = re.search(r'^\s*(\d+) ' + re.escape(role) + r' ' + re.escape(label) + r'\s*$', snapshot['treeText'], re.M)
        if not match:
            raise RuntimeError(f'{stage}: enabled {role} {label} missing')
        return int(match[1])

    def sheet_root(snapshot):
        lines = [line for line in snapshot['treeText'].splitlines() if re.match(r'^\d+ ', line)]
        return bool(lines and re.match(r'^\d+ sheet(?: |$)', lines[0]))

    try:
        fixture = subprocess.Popen([str(fixture_binary)], env=env, stdout=log, stderr=log)
        def find_main():
            matches = [w for w in windows('wait-main') if w['title'] == 'Shifu File Picker Fixture']
            return matches[0]['id'] if len(matches) == 1 else None
        main_id = wait('main_ready', find_main)
        before = observe('main-before', main_id)
        choose = control(before, 'button', 'Choose test file')
        initial_ids = {w['id'] for w in windows('initial-windows')}
        call('choose-file', 'click', '--window-id', main_id, '--element-index', choose)
        def find_panel():
            for w in windows('wait-panel'):
                if w['id'] not in initial_ids:
                    snapshot = observe('panel-candidate', w['id'])
                    if sheet_root(snapshot):
                        return w['id'], snapshot
            return None
        panel_id, panel = wait('open_panel_ready', find_panel)
        assert panel['window']['id'] == panel_id and sheet_root(panel), 'panel must be its own sheet snapshot'
        panel_ids = {w['id'] for w in windows('before-go-to')}
        call('go-to-path', 'hotkey', '--window-id', panel_id, '--key', 'CmdOrCtrl+Shift+G', '--restore-window')
        def find_go_to():
            for w in windows('wait-go-to'):
                if w['id'] not in panel_ids:
                    snapshot = observe('go-to-candidate', w['id'])
                    focused = snapshot.get('focusedElementId')
                    if sheet_root(snapshot) and focused is not None:
                        line = re.search(r'^\s*' + str(focused) + r' text field \(settable\)', snapshot['treeText'], re.M)
                        if line:
                            return w['id'], focused
            return None
        go_id, field = wait('go_to_ready', find_go_to)
        changed = call('set-sample-path', 'set-value', '--window-id', go_id, '--element-index', field,
                       '--value-stdin', text=str(sample))
        assert changed['result']['action']['verification']['state'] == 'verified', 'path value not verified'
        readback = observe('go-to-readback', go_id)
        assert str(sample) in readback['treeText'], 'absolute path not read back'
        call('commit-path', 'press-key', '--window-id', go_id, '--key', 'Return', '--restore-window')
        wait('go_to_closed', lambda: True if go_id not in {w['id'] for w in windows('wait-go-to-close')} else None)
        def selected_sample():
            snapshot = observe('sample-candidate', panel_id)
            if not sheet_root(snapshot):
                return None
            rows = []
            target_rows = []
            selected_file_rows = set()
            filename_pattern = re.compile(r'^\d+ text field[^\n]*, Value: ' + re.escape(sample.name) + r'(?:, Secondary Actions:[^\n]*)?$')
            for line in snapshot['treeText'].splitlines():
                content = line.lstrip()
                if not re.match(r'^\d+ ', content):
                    continue
                depth = len(line) - len(content)
                while rows and rows[-1][0] >= depth:
                    rows.pop()
                row = re.match(r'^(\d+) outline row(?: |$)', content)
                if row:
                    rows.append((depth, int(row[1]), '(selected)' in content))
                elif re.match(r'^\d+ text field(?: |$)', content) and rows:
                    owner = rows[-1]
                    if owner[2]:
                        selected_file_rows.add(owner[1])
                    if filename_pattern.match(content):
                        target_rows.append(owner)
            # Sidebar selection does not count: only outline rows owning file text fields.
            selected_target = (len(target_rows) == 1 and target_rows[0][2]
                               and selected_file_rows == {target_rows[0][1]})
            enabled = re.search(r'^\s*(\d+) button Open\s*$', snapshot['treeText'], re.M)
            return (snapshot, int(enabled[1])) if selected_target and enabled else None
        selected, open_button = wait('sample_selected_and_open_enabled', selected_sample)
        call('confirm-open', 'click', '--window-id', panel_id, '--element-index', open_button)
        def completed():
            snapshot = observe('result-candidate', main_id)
            tree = snapshot['treeText']
            filename = re.search(r'^\s*\d+ text Selected: ' + re.escape(sample.name) + r'\s*$', tree, re.M)
            count = re.search(r'^\s*\d+ text Selections: 1\s*$', tree, re.M)
            return snapshot if filename and count else None
        wait('selected_file_and_single_submission', completed)
        assert sample.read_text() == 'Disposable file picker acceptance\n'
        report.update(status='passed', stage='completed', selectedFile=sample.name, selections=1)
    except Exception as error:
        report.update(status='failed', stage=stage, errorType=type(error).__name__, message=str(error))
    finally:
        if fixture is not None:
            fixture.terminate()
            try:
                fixture.wait(timeout=5)
            except subprocess.TimeoutExpired:
                fixture.kill()
                fixture.wait(timeout=5)
            report['fixtureStopped'] = fixture.poll() is not None
        log.close()
        try:
            stop = subprocess.run([cli, 'computer', 'stop'], capture_output=True, text=True, timeout=15)
            (root / 'stop.stdout').write_text(stop.stdout)
            (root / 'stop.stderr').write_text(stop.stderr)
            report['brokerStopExitCode'] = stop.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            report['brokerCleanupError'] = str(error)
        report['durationMs'] = round((time.monotonic() - started) * 1000)
        report['cliCallCount'] = calls
        (root / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps(report, ensure_ascii=False))
    return 0 if report['status'] == 'passed' and report.get('fixtureStopped') and report.get('brokerStopExitCode') == 0 else 1


if __name__ == '__main__':
    raise SystemExit(main())
