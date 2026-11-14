#!/usr/bin/env python3
"""Reuse one compiler cache per worktree/component; retire only caches created by this tool."""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time


MARKER = '.shifu-cache.json'
TAG = 'Signature: 8a477f597d28d172789f06886806bc55\n# Shifu managed compiler cache. No source, reports, or handoffs.\n'
KINDS = ('cli', 'desktop', 'remote')


def git(*args):
    return subprocess.check_output(['git', '--no-optional-locks', *args], text=True, stderr=subprocess.PIPE, timeout=10).strip()


def context():
    source = Path(git('rev-parse', '--show-toplevel')).resolve()
    common = Path(git('rev-parse', '--path-format=absolute', '--git-common-dir')).resolve()
    gitdir = Path(git('rev-parse', '--absolute-git-dir')).resolve()
    base = common.parent / 'target/shifu-build-cache.noindex'
    return source, common, gitdir, base


def cache_key(source, gitdir, kind):
    identity = f'{source}\n{gitdir}\n{kind}'.encode()
    return kind + '-' + hashlib.sha256(identity).hexdigest()[:20]


def write_json(path, value):
    temporary = path.with_name(path.name + f'.{os.getpid()}.tmp')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2))
    temporary.replace(path)


@contextmanager
def lock(base, key, blocking=False):
    directory = base / '.locks'
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / (key + '.lock')).open('a') as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
        except BlockingIOError:
            yield False
            return
        try:
            yield True
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


def metadata(path, common):
    if path.is_symlink() or path.resolve() != path:
        raise ValueError('cache path is a symlink')
    data = json.loads((path / MARKER).read_text())
    try:
        valid = (data['version'] == 1 and data['repository'] == str(common)
                 and data['kind'] in KINDS and math.isfinite(data['last_used'])
                 and cache_key(data['source'], data['gitdir'], data['kind']) == path.name
                 and (path / 'CACHEDIR.TAG').read_text().startswith(TAG.splitlines()[0]))
    except (KeyError, TypeError) as error:
        raise ValueError(f'缓存归属标记不完整：{path}') from error
    if not valid:
        raise ValueError('cache ownership does not match')
    return data


def open_files():
    command = shutil.which('lsof') or '/usr/sbin/lsof'
    try:
        out = subprocess.run([command, '-nP', '-Fn'], text=True, capture_output=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if out.returncode != 0 or out.stderr.strip():
        return None
    return [line[1:] for line in out.stdout.splitlines() if line.startswith('n')]


def source_worktrees():
    return [Path(field[9:]).resolve() for field in git('worktree', 'list', '--porcelain', '-z').split('\0') if field.startswith('worktree ')]


def occupied(path, files, sources):
    if any(source.is_relative_to(path) for source in sources):
        return 'source-worktree'
    if files is None:
        return 'usage-unavailable'
    if any(name == str(path) or name.startswith(str(path) + '/') for name in files):
        return 'open-files'
    return None


def size(path):
    out = subprocess.run(['du', '-sk', str(path)], capture_output=True, text=True, check=True, timeout=20)
    return int(out.stdout.split()[0]) * 1024


def plan(base, common, budget, idle_days, protected=()):
    entries, skipped = [], []
    files, sources = open_files(), source_worktrees()
    for path in sorted(base.iterdir()) if base.exists() else []:
        if not path.is_dir() or path.name.startswith('.'):
            continue
        try:
            data = metadata(path, common)
        except (OSError, ValueError, KeyError, TypeError):
            skipped.append(str(path))
            continue
        with lock(base, path.name) as acquired:
            reason = 'building' if not acquired else occupied(path, files, sources)
            if path.name in protected:
                reason = 'current-build'
            try:
                bytes_used = size(path)
            except (OSError, subprocess.SubprocessError, ValueError):
                reason, bytes_used = 'size-unavailable', 0
            entries.append({'path': str(path), 'kind': data['kind'], 'bytes': bytes_used,
                            'last_used': data['last_used'], 'action': 'keep', 'reason': reason})
    total = sum(row['bytes'] for row in entries)
    remaining = total
    for row in sorted(entries, key=lambda row: row['last_used']):
        if row['reason'] is not None:
            continue
        expired = time.time() - row['last_used'] >= idle_days * 86400
        if expired or remaining > budget:
            row.update(action='remove', reason='expired' if expired else 'budget')
            remaining -= row['bytes']
        else:
            row['reason'] = 'reusable'
    return {'entries': entries, 'skipped_unmanaged': skipped, 'managed_bytes': total,
            'budget_bytes': budget, 'projected_over_budget_bytes': max(0, remaining - budget)}


def prune(base, common, budget, idle_days, apply=False, protected=()):
    report = plan(base, common, budget, idle_days, protected)
    report['removed'] = []
    if not apply:
        return report
    for row in report['entries']:
        if row['action'] != 'remove':
            continue
        path = Path(row['path'])
        with lock(base, path.name) as acquired:
            try:
                data = metadata(path, common)
                reason = ('building' if not acquired else
                          occupied(path, open_files(), source_worktrees()))
                if data['last_used'] != row['last_used']:
                    reason = 'used-since-plan'
                if reason:
                    row.update(action='keep', reason=reason)
                    continue
                # Only this exact, freshly verified, tool-owned compiler directory is removed.
                print(f'回收新入口的闲置编译缓存：{path} ({row["reason"]})', file=sys.stderr)
                shutil.rmtree(path)
                report['removed'].append(str(path))
            except (OSError, ValueError, KeyError, TypeError) as error:
                row.update(action='keep', reason='cleanup-failed', error=str(error))
                print(f'缓存未能回收，已保留：{path}：{error}', file=sys.stderr)
    remaining = sum(row['bytes'] for row in report['entries'] if row['path'] not in report['removed'])
    report['over_budget_bytes'] = max(0, remaining - budget)
    if base.exists():
        write_json(base / 'last-cleanup.json', report)
    return report


def stop(child):
    if child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)


def run(args, ctx):
    source, common, gitdir, base = ctx
    key = cache_key(source, gitdir, args.kind)
    path = base / key
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        raise ValueError('run 后需用 -- 给出构建命令')
    if any(arg == '--target-dir' or arg.startswith('--target-dir=') for arg in command):
        raise ValueError('本入口统一管理 CARGO_TARGET_DIR，请移除额外的 --target-dir')
    with lock(base, key, blocking=True):
        if path.exists():
            data = metadata(path, common)
        else:
            path.mkdir()
            (path / 'CACHEDIR.TAG').write_text(TAG)
            data = {'version': 1, 'repository': str(common), 'source': str(source),
                    'gitdir': str(gitdir), 'kind': args.kind}
        data.update(last_used=time.time(), status='building')
        write_json(path / MARKER, data)
        env = dict(os.environ, CARGO_TARGET_DIR=str(path),
                   SHIFU_REMOTE_BUILD_DIR=str(path / 'remote'), SHIFU_CRYPTO_BUILD_DIR=str(path / 'crypto'),
                   SHIFU_REMOTE_TOOLS_DIR=str(path / 'tools'), SHIFU_BUILD_STAGE_ROOT=str(path / 'staging'))
        print(f'复用编译目录：{path}', file=sys.stderr)
        child = None
        result = 1
        try:
            child = subprocess.Popen(command, env=env, start_new_session=True)
            result = child.wait(timeout=args.timeout_seconds)
        except subprocess.TimeoutExpired:
            result = 124
            print('构建超时，停止本次构建进程；缓存保留，可修正后续跑。', file=sys.stderr)
        except KeyboardInterrupt:
            result = 130
        finally:
            if child is not None:
                stop(child)
            data.update(last_used=time.time(), status='cached', last_exit=result)
            write_json(path / MARKER, data)
        if result:
            return result
        report = prune(base, common, args.budget_bytes, args.idle_days, apply=True, protected=(key,))
        if report['over_budget_bytes']:
            print('缓存超过预算，但正在使用或无法核实的内容已保留；用 plan 查看原因。', file=sys.stderr)
    return 0


def number(value):
    parsed = float(value)
    if not math.isfinite(parsed) or parsed < 0:
        raise argparse.ArgumentTypeError('请输入有限的非负数')
    return parsed


def interrupted(_signal, _frame):
    raise KeyboardInterrupt()


def main():
    parser = argparse.ArgumentParser(description='同一工作区复用编译缓存；仅回收本入口创建的缓存。历史target不纳管。')
    commands = parser.add_subparsers(dest='action', required=True)
    for action in ['path', 'run', 'plan', 'prune']:
        command = commands.add_parser(action)
        if action in ['path', 'run']:
            command.add_argument('--kind', choices=KINDS, required=True)
        if action != 'path':
            command.add_argument('--budget-gib', type=number, default=20)
            command.add_argument('--idle-days', type=number, default=7)
        if action == 'run':
            command.add_argument('--timeout-seconds', type=number, default=1200)
            command.add_argument('command', nargs=argparse.REMAINDER)
        if action == 'prune':
            command.add_argument('--apply', action='store_true', help='执行清理；不带此项只预览')
    args = parser.parse_args()
    ctx = context()
    source, common, gitdir, base = ctx
    if args.action == 'path':
        print(base / cache_key(source, gitdir, args.kind))
        return 0
    args.budget_bytes = int(args.budget_gib * 1024 ** 3)
    if args.action == 'run':
        if args.timeout_seconds == 0:
            raise ValueError('构建超时须大于0秒')
        previous = signal.signal(signal.SIGTERM, interrupted)
        try:
            return run(args, ctx)
        finally:
            signal.signal(signal.SIGTERM, previous)
    report = prune(base, common, args.budget_bytes, args.idle_days, apply=getattr(args, 'apply', False))
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 1 if any(row['reason'] == 'cleanup-failed' for row in report['entries']) else 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'build-cache: {error}', file=sys.stderr)
        sys.exit(1)
