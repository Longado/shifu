#!/usr/bin/env python3
"""Opt-in real AX acceptance, targeting only an owned background AppKit fixture.

Usage: verify-background-only.py NEW_RESULT_DIR COMPILED_FIXTURE COMPILED_PROBE
Set SHIFU_CLI and an isolated SHIFU_COMPUTER_HOME. A helper override is optional.
No foreground restoration, screenshots, clipboard content or user app input.
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
probe = str(Path(sys.argv[3]).resolve())
fixture = None
result = {'completed': False, 'scope': 'owned fixture only; no browser claim', 'metadata_samples': {}}


def sample(name):
    state = json.loads(subprocess.check_output([probe], text=True, timeout=10))
    result['metadata_samples'][name] = state
    return state


def call(name, *args, rejected=False):
    command = [cli, 'computer', *map(str, args), '--app', f'pid:{fixture.pid}', '--no-screenshot', '--background-only']
    run = subprocess.run(command, capture_output=True, text=True, timeout=60)
    (root / f'{name}.stderr').write_text(run.stderr)
    (root / f'{name}.json').write_text(run.stdout)
    if rejected:
        assert run.returncode != 0 and '--background-only' in run.stderr, (name, run.stderr)
        return
    response = json.loads(run.stdout)
    assert run.returncode == 0 and response['ok'], (name, response)
    return response['result']


try:
    baseline = sample('before_fixture')
    with (root / 'fixture.log').open('w') as log:
        env = dict(os.environ)
        env.pop('SHIFU_FIXTURE_OBSERVATION', None)
        fixture = subprocess.Popen([str(Path(sys.argv[2]).resolve())], env=env, stdout=log, stderr=log)
    until = time.monotonic() + 10
    while True:
        windows = call('windows', 'list-windows').get('windows', [])
        if windows:
            break
        assert time.monotonic() < until, 'fixture did not expose its background window'
        time.sleep(.15)
    assert len(windows) == 1, windows
    window = windows[0]['id']
    snapshot = call('before', 'get-app-state', '--window-id', window)['snapshot']
    match = re.search(r'^\s*(\d+) text field[^\n]*(?:测试输入框|初始文字)', snapshot['treeText'], re.M)
    assert match, snapshot['treeText']
    index = match[1]
    assigned = '严格后台：中文 English 123'
    run = subprocess.run([
        cli, 'computer', 'set-value', '--app', f'pid:{fixture.pid}', '--window-id', str(window),
        '--element-index', index, '--value-stdin', '--background-only', '--no-screenshot',
    ], input=assigned, capture_output=True, text=True, timeout=60)
    (root / 'assignment.json').write_text(run.stdout)
    (root / 'assignment.stderr').write_text(run.stderr)
    response = json.loads(run.stdout)
    assert run.returncode == 0 and response['ok'], response
    action = response['result']['action']
    assert action['backgroundOnly'] is True and action['path'] == 'accessibility', action
    assert action['verification']['state'] == 'verified', action
    assert assigned in response['result']['snapshot']['treeText'], response
    sample('after_assignment')
    call('reject-restore', 'get-app-state', '--window-id', window, '--restore-window', rejected=True)
    call('reject-click', 'click', '--window-id', window, '--element-index', index, rejected=True)
    call('reject-paste', 'paste-text', '--text', 'MUST-NOT-BE-PASTED', rejected=True)
    after = call('after', 'get-app-state', '--window-id', window)['snapshot']
    assert assigned in after['treeText'] and '点击次数：0' in after['treeText'], after
    sample('after_rejections')
    for name, state in result['metadata_samples'].items():
        assert state['frontmostAppPid'] != fixture.pid, name
    result['completed'] = True
except Exception as error:
    result['error'] = str(error)
finally:
    if fixture is not None:
        fixture.terminate()
        fixture.wait(timeout=10)
    stopped = subprocess.run([cli, 'computer', 'stop'], capture_output=True, text=True, timeout=20)
    result['isolated_broker_stopped'] = stopped.returncode == 0
    result['fixture_stopped'] = fixture is None or fixture.poll() is not None
    sample('after_cleanup')
    result['sampled_metadata_unchanged'] = all(state == baseline for state in result['metadata_samples'].values())
    (root / 'result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({k: result[k] for k in ['completed', 'sampled_metadata_unchanged'] if k in result}))
if not result['completed']:
    raise SystemExit(result.get('error', 'acceptance failed'))
