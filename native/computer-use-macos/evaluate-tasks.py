#!/usr/bin/env python3
"""Run bounded scripted UI tasks; this does not measure autonomous-agent success."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

from task_pages import cleanup_pages

TASK_IDS = ('finder_rename', 'browser_target_and_restore')


def summarize(records):
    counts = {}
    for record in records:
        for task in record.get('tasks', []):
            task_id, status = task.get('taskId'), task.get('status')
            if task_id in TASK_IDS and status in ('passed', 'failed', 'not_run'):
                counts.setdefault(task_id, {'passed': 0, 'failed': 0, 'not_run': 0})[status] += 1
    return {'evaluationKind': 'scripted_ui_tasks', 'tasks': counts,
            'humanIntervention': 'unknown', 'foregroundInterruptions': 'unknown'}


def run_suite(argv, result_dir, timeout_seconds):
    result_dir = Path(result_dir)
    result_dir.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    timed_out = False
    launch_error = None
    stdout_path = result_dir.with_name(result_dir.name + '.stdout.log')
    stderr_path = result_dir.with_name(result_dir.name + '.stderr.log')
    with stdout_path.open('w') as stdout, stderr_path.open('w') as stderr:
        try:
            process = subprocess.Popen(argv, stdout=stdout, stderr=stderr, start_new_session=True)
            try:
                process.wait(timeout=timeout_seconds)
            except subprocess.TimeoutExpired:
                timed_out = True
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
            exit_code = process.returncode
        except OSError as error:
            launch_error = str(error)
            exit_code = None
    evidence = {'result': str(result_dir / 'result.json'), 'stdout': str(stdout_path), 'stderr': str(stderr_path), 'ownedWindows': str(result_dir / 'owned-windows.json'), 'ownedPages': str(result_dir / 'owned-pages.json')}
    report = {}
    report_error = None
    try:
        report = json.loads((result_dir / 'result.json').read_text())
        if not isinstance(report, dict):
            raise ValueError('child report must be an object')
    except (OSError, ValueError) as error:
        report_error = str(error)
        report = {}
    tasks = report.get('tasks')
    if not isinstance(tasks, list):
        tasks = []
    tasks = [task for task in tasks if isinstance(task, dict) and task.get('taskId') in TASK_IDS]
    known = {task['taskId'] for task in tasks}
    tasks += [{'taskId': task_id, 'status': 'not_run', 'failedStage': 'process',
               'errorCode': 'report_unavailable', 'humanIntervention': 'unknown'}
              for task_id in TASK_IDS if task_id not in known]
    return {'process': {'exitCode': exit_code, 'timedOut': timed_out, 'launchError': launch_error},
            'tasks': tasks, 'durationMs': round((time.monotonic() - started) * 1000),
            'evidence': evidence, 'reportError': report_error, 'cleanup': report.get('cleanup', []),
            'cleanupErrors': report.get('cleanupErrors', []),
            'windowCleanupState': 'unknown' if timed_out or report_error else ('reported' if 'cleanup' in report else 'unknown')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output_dir', type=Path)
    parser.add_argument('--runs', type=int, default=1, choices=range(1, 11))
    parser.add_argument('--cli', default='shifu')
    args = parser.parse_args()
    root = args.output_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    records = []
    for index in range(args.runs):
        result_dir = root / f'run-{index + 1:02d}'
        with tempfile.TemporaryDirectory(prefix='scu-', dir='/tmp') as computer_home:
            env = dict(os.environ, SHIFU_CLI=args.cli, SHIFU_COMPUTER_HOME=computer_home)
            command = ['/usr/bin/env', *[f'{k}={env[k]}' for k in ('SHIFU_CLI', 'SHIFU_COMPUTER_HOME')],
                       sys.executable, str(Path(__file__).with_name('verify-real-tasks.py')), str(result_dir)]
            record = run_suite(command, result_dir, 90)
            inventory = result_dir / 'owned-pages.json'
            if inventory.exists():
                try:
                    record['pageCleanup'] = cleanup_pages(json.loads(inventory.read_text()))
                    if record['pageCleanup']['remaining']:
                        record['cleanupErrors'].append({'stage': 'close_pages_after_child', 'message': 'owned Safari URLs remain open'})
                except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                    record['cleanupErrors'].append({'stage': 'close_pages_after_child', 'message': str(error)})
            try:
                stop = subprocess.run([args.cli, 'computer', 'stop'], env=env, capture_output=True, text=True, timeout=15)
                record['brokerCleanup'] = {'exitCode': stop.returncode, 'stdout': stop.stdout, 'stderr': stop.stderr}
            except (OSError, subprocess.TimeoutExpired) as error:
                record['brokerCleanup'] = {'error': str(error)}
            records.append(record)
        report = summarize(records)
        report['runs'] = records
        (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps(summarize(records), ensure_ascii=False))
    return 0 if all(r['process']['exitCode'] == 0 and not r['process']['timedOut'] and not r['cleanupErrors'] for r in records) else 1


if __name__ == '__main__':
    raise SystemExit(main())
