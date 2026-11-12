#!/usr/bin/env python3
"""Compare identical disposable AppKit sequences, not autonomous-agent success."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import uuid


def provider_order(index):
    return ['shifu', 'orca'] if index % 2 == 0 else ['orca', 'shifu']


def run_command(command, root, name, env, timeout=45):
    started = time.monotonic()
    root = Path(root)
    timed_out = False
    error = None
    with (root / (name + '.stdout')).open('w') as stdout, (root / (name + '.stderr')).open('w') as stderr:
        try:
            child = subprocess.Popen(command, env=env, stdout=stdout, stderr=stderr)
            try:
                child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                child.kill()
                child.wait(timeout=5)
            exit_code = child.returncode
        except OSError as exc:
            error = str(exc)
            exit_code = None
    raw = (root / (name + '.stdout')).read_text()
    try:
        response = json.loads(raw)
    except ValueError:
        response = None
    tree = response.get('result', {}).get('snapshot', {}).get('treeText', '') if isinstance(response, dict) else ''
    record = {'exitCode': exit_code, 'timedOut': timed_out, 'response': response,
              'treeBytes': len(tree.encode('utf-8')) if isinstance(tree, str) else 0,
              'elapsedMs': round((time.monotonic()-started)*1000), 'launchError': error,
              'stdout': str(root/(name+'.stdout')), 'stderr': str(root/(name+'.stderr'))}
    (root/(name+'.json')).write_text(json.dumps(record, ensure_ascii=False, indent=2))
    return record


def run_sequence(provider, cli, pid, root, env, session, call=run_command):
    records = []
    stage = 'list-windows'
    started = time.monotonic()
    def invoke(name, method, *options):
        nonlocal stage
        stage = name
        command = [cli, 'computer', method, *map(str, options), '--app', f'pid:{pid}', '--json']
        if method != 'list-windows':
            command += ['--session', session, '--no-screenshot']
        if provider == 'orca':
            command += ['--environment', 'local']
        record = call(command, root, name, env, 45)
        record['stage'] = name
        record['method'] = method
        records.append(record)
        response = record['response']
        if record['timedOut'] or record['exitCode'] != 0 or not isinstance(response, dict) or response.get('ok') is not True:
            raise RuntimeError(f'{name}: command failed; see raw response')
        return response['result']
    def element(tree, role, label):
        match = re.search(r'^\s*(\d+) ' + re.escape(role) + r' .*' + re.escape(label) + r'.*$', tree, re.M)
        if not match:
            raise RuntimeError(f'{stage}: missing {label}')
        return int(match[1])
    try:
        windows = invoke('windows', 'list-windows')['windows']
        window = next(w['id'] for w in windows if w['title'] == 'Shifu Computer Use 测试窗口')
        before = invoke('observe-before', 'get-app-state', '--window-id', window)['snapshot']['treeText']
        assert '点击次数：0' in before, 'fixture did not start at zero'
        invoke('increment', 'click', '--window-id', window, '--element-index', element(before, 'button', '增加计数'))
        clicked = invoke('observe-count', 'get-app-state', '--window-id', window)['snapshot']['treeText']
        assert '点击次数：1' in clicked, 'count did not become one'
        invoke('set-value', 'set-value', '--window-id', window, '--element-index', element(clicked, 'text field', '测试输入框'), '--value', '中文对照')
        changed = invoke('observe-value', 'get-app-state', '--window-id', window)['snapshot']['treeText']
        assert '中文对照' in changed, 'Chinese value not observed'
        status, message, failed_stage = 'passed', None, None
    except (RuntimeError, AssertionError, KeyError, StopIteration, TypeError) as exc:
        status, message, failed_stage = 'failed', str(exc), stage
    return {'provider':provider, 'status':status, 'failedStage':failed_stage, 'message':message,
            'elapsedMs':round((time.monotonic()-started)*1000), 'steps':records,
            'cliCallCount':len(records), 'actionCount':sum(r['method'] in ('click','set-value') for r in records),
            'allResponseTreeBytes':sum(r['treeBytes'] for r in records),
            'fullObservationTreeBytes':sum(r['treeBytes'] for r in records if r['method']=='get-app-state'),
            'humanIntervention':'unknown', 'foregroundInterruptions':'unknown'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--fixture', required=True, type=Path)
    parser.add_argument('--shifu', required=True, type=Path)
    parser.add_argument('--orca', required=True, type=Path)
    parser.add_argument('--runs', type=int, choices=range(1,6), default=1)
    args = parser.parse_args()
    for path in (args.fixture,args.shifu,args.orca):
        if not path.is_absolute() or not path.is_file(): parser.error('fixture and CLI paths must be existing absolute files')
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base_env = {k:v for k,v in os.environ.items() if not k.startswith('ORCA_') and k not in ('SHIFU_ID','TMUX','TMUX_PANE')}
    report = {'evaluationKind':'scripted_fixture_tasks', 'sequence':['observe','increment_once','observe_count','set_Chinese_value','observe_value'],
              'notes':'Same explicit full observations for both providers. All-response bytes include native action delta/full differences and measure repeated readback volume, not complete-tree coverage or model tokens. No screenshots or foreground restoration requested.', 'runs':[]}
    for provider in ['shifu','orca']:
        cli = str(getattr(args,provider))
        version = run_command([cli,'--version'],root,provider+'-version',base_env,10)
        report[provider+'Version'] = (root/(provider+'-version.stdout')).read_text().strip()
        report[provider+'VersionProcess'] = version
    for index in range(args.runs):
        for provider in provider_order(index):
            directory = root/f'run-{index+1:02d}-{provider}'
            directory.mkdir()
            cli = str(getattr(args,provider))
            with tempfile.TemporaryDirectory(prefix='scmp-',dir='/tmp') as home:
                env = dict(base_env, SHIFU_COMPUTER_HOME=home)
                fixture_env = {k:v for k,v in env.items() if k not in ('SHIFU_FIXTURE_OBSERVATION','SHIFU_FIXTURE_COUNTER_PATH')}
                with (directory/'fixture.log').open('w') as log:
                    fixture = subprocess.Popen([str(args.fixture)],env=fixture_env,stdout=log,stderr=log)
                    try:
                        time.sleep(.5)
                        result = run_sequence(provider,cli,fixture.pid,directory,env,'compare-'+uuid.uuid4().hex)
                    finally:
                        fixture.terminate()
                        try: fixture.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            fixture.kill();fixture.wait(timeout=5)
                result['fixtureStopped'] = fixture.poll() is not None
                result['round'] = index+1
                if provider == 'shifu':
                    result['brokerCleanup'] = run_command([cli,'computer','stop'],directory,'stop-own-broker',env,15)
                report['runs'].append(result)
                (root/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
    print(json.dumps({'evaluationKind':report['evaluationKind'],'runs':[{k:r[k] for k in ('round','provider','status','failedStage','elapsedMs','cliCallCount','allResponseTreeBytes','fullObservationTreeBytes')} for r in report['runs']]},ensure_ascii=False))
    return 0 if all(r['status']=='passed' for r in report['runs']) else 1


if __name__ == '__main__':
    raise SystemExit(main())
