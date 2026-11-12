#!/usr/bin/env python3
"""Exercise incremental output and complete observations against one disposable PID."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import uuid

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=False)
cli = os.environ.get('SHIFU_CLI', 'shifu')
assert Path(os.environ['SHIFU_COMPUTER_HOME']).resolve() != Path.home() / '.shifu/computer'
env = {k: v for k, v in os.environ.items() if k not in ('SHIFU_FIXTURE_OBSERVATION', 'SHIFU_FIXTURE_COUNTER_PATH')}
session = 'diff-' + uuid.uuid4().hex
results = {}
log = (root / 'fixture.log').open('w')
fixture = subprocess.Popen([str(Path(sys.argv[2]).resolve())], env=env, stdout=log, stderr=log)


def call(name, method, *options):
    process = subprocess.run([cli, 'computer', method, '--app', f'pid:{fixture.pid}',
                              '--session', session, '--no-screenshot', *map(str, options)],
                             capture_output=True, text=True, timeout=30)
    (root / (name + '.stdout')).write_text(process.stdout)
    (root / (name + '.stderr')).write_text(process.stderr)
    response = json.loads(process.stdout)
    assert process.returncode == 0 and response['ok'], (name, response)
    return response['result']


def button(tree):
    return int(re.search(r'^\s*(\d+) button 增加计数\s*$', tree, re.M)[1])


try:
    until = time.monotonic() + 10
    while True:
        windows = call('windows', 'list-windows')['windows']
        if windows:
            break
        assert time.monotonic() < until, 'fixture window not ready'
        time.sleep(.1)
    window = windows[0]['id']
    before = call('before', 'get-app-state', '--window-id', window)['snapshot']
    index = button(before['treeText'])
    changed = call('changed', 'click', '--window-id', window, '--element-index', index)['snapshot']
    assert changed.get('treeDiff') is True, changed['treeText']
    assert changed['baselineSnapshotId'] == before['id']
    assert '点击次数：1' in changed['treeText'] and '~ ' in changed['treeText']
    assert len(changed['treeText'].encode()) < len(before['treeText'].encode())
    results['changed_action_is_smaller_delta'] = True

    chained = call('chained', 'click', '--window-id', window, '--element-index', index)['snapshot']
    assert chained.get('treeDiff') is True and chained['baselineSnapshotId'] == changed['id']
    assert '点击次数：2' in chained['treeText']
    results['next_action_uses_reconstructable_delta_baseline'] = True

    full = call('full-read', 'get-app-state', '--window-id', window)['snapshot']
    assert not full.get('treeDiff') and button(full['treeText']) == index
    results['read_is_complete_with_stable_indexes'] = True

    filtered = call('filtered-read', 'get-app-state', '--window-id', window, '--match', '测试输入框')['snapshot']
    assert filtered['view']['filtered'] and '增加计数' not in filtered['treeText']
    following = call('after-filter', 'click', '--window-id', window, '--element-index', index)['snapshot']
    assert not following.get('treeDiff') and '点击次数：3' in following['treeText']
    assert button(following['treeText']) == index
    results['filtered_view_is_not_a_full_baseline'] = True

    explicit = call('explicit-full', 'click', '--window-id', window, '--element-index', index, '--full-tree')['snapshot']
    assert not explicit.get('treeDiff') and '点击次数：4' in explicit['treeText']
    assert button(explicit['treeText']) == index
    results['explicit_full_tree'] = True

    waited = call('unchanged-condition', 'click', '--window-id', window, '--element-index', index,
                  '--wait-for', '仅测试窗口', '--timeout-ms', 0)
    assert waited['postcondition']['state'] == 'met'
    assert not waited['snapshot'].get('treeDiff') and '点击次数：5' in waited['snapshot']['treeText']
    results['unchanged_text_condition_uses_full_tree'] = True

    gone = call('gone-condition', 'click', '--window-id', window, '--element-index', index,
                '--wait-for-gone', 'NOT-IN-THIS-FIXTURE', '--timeout-ms', 0)
    assert gone['postcondition']['state'] == 'met' and not gone['snapshot'].get('treeDiff')
    assert '点击次数：6' in gone['snapshot']['treeText']
    results['disappearance_condition_uses_full_tree'] = True
finally:
    fixture.terminate()
    try:
        fixture.wait(timeout=5)
    except subprocess.TimeoutExpired:
        fixture.kill()
        fixture.wait(timeout=5)
    log.close()
    stop = subprocess.run([cli, 'computer', 'stop'], capture_output=True, text=True, timeout=15)
    (root / 'stop.stdout').write_text(stop.stdout)
    results['fixture_stopped'] = fixture.poll() is not None
    (root / 'result.json').write_text(json.dumps(results, indent=2))
print(json.dumps(results))
