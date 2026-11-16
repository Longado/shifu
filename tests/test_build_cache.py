import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/build-cache.py'
PREPARE = Path(__file__).resolve().parents[1] / 'scripts/prepare-desktop.sh'
WRITE = 'import os,pathlib; p=pathlib.Path(os.environ["CARGO_TARGET_DIR"]); f=p/"artifact"; f.write_text(f.read_text()+"x" if f.exists() else "x")'


class BuildCacheTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='shifu-build-cache-')
        self.root = Path(self.temp.name) / 'repo with 空格'
        self.root.mkdir()
        self.git('init', '-b', 'main')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        (self.root / '.gitignore').write_text('/target\n')
        (self.root / 'source.txt').write_text('source must survive')
        self.git('add', '.')
        self.git('commit', '-m', 'initial')

    def tearDown(self):
        self.temp.cleanup()

    def git(self, *args):
        return subprocess.check_output(['git', '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', *args], cwd=self.root, stderr=subprocess.DEVNULL, text=True).strip()

    def command(self, *args, cwd=None, ok=True):
        self.assertTrue(SCRIPT.is_file(), '缺少可复用、可回收的构建入口')
        out = subprocess.run([sys.executable, str(SCRIPT), *args], cwd=cwd or self.root, capture_output=True, text=True, timeout=15)
        if ok:
            self.assertEqual(out.returncode, 0, out.stderr)
        return out

    def path(self, kind='cli', cwd=None):
        return Path(self.command('path', '--kind', kind, cwd=cwd).stdout.strip())

    def build(self, kind='cli', *options):
        return self.command('run', '--kind', kind, *options, '--', sys.executable, '-c', WRITE)

    def test_same_worktree_reuses_cache_and_parallel_worktree_is_isolated(self):
        target = self.path()
        self.build()
        self.build()
        self.assertEqual((target / 'artifact').read_text(), 'xx')
        child = Path(self.temp.name) / 'parallel'
        self.git('worktree', 'add', '-b', 'parallel', str(child))
        self.assertNotEqual(target, self.path(cwd=child))
        self.assertTrue(target.is_relative_to(self.root.resolve() / 'target'))
        self.assertEqual((self.root / 'source.txt').read_text(), 'source must survive')

    def test_budget_cleanup_only_removes_owned_idle_cache_and_preview_is_read_only(self):
        old = self.path()
        self.build()
        legacy = self.root / 'target/legacy'
        legacy.mkdir()
        (legacy / 'draft.txt').write_text('not a managed cache')
        unmanaged = old.parent / 'unmanaged'
        unmanaged.mkdir()
        (unmanaged / 'draft.txt').write_text('keep')
        plan = json.loads(self.command('plan', '--budget-gib', '0').stdout)
        self.assertTrue(any(row['path'] == str(old) and row['action'] == 'remove' for row in plan['entries']))
        self.assertTrue((old / 'artifact').exists())
        self.build('remote', '--budget-gib', '0')
        self.assertFalse(old.exists())
        self.assertTrue((self.path('remote') / 'artifact').exists())
        self.assertTrue((legacy / 'draft.txt').exists())
        self.assertTrue((unmanaged / 'draft.txt').exists())

    def test_open_artifacts_and_running_builds_are_kept(self):
        target = self.path()
        self.build()
        with (target / 'artifact').open():
            report = json.loads(self.command('prune', '--apply', '--budget-gib', '0').stdout)
            self.assertTrue(target.exists())
            self.assertTrue(any(row['reason'] == 'open-files' for row in report['entries']))
        ready = self.root / 'ready'
        code = f'import pathlib,time; pathlib.Path({str(ready)!r}).write_text("ready"); time.sleep(1.5)'
        proc = subprocess.Popen([sys.executable, str(SCRIPT), 'run', '--kind', 'cli', '--', sys.executable, '-c', code], cwd=self.root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            until = time.monotonic() + 5
            while not ready.exists() and time.monotonic() < until:
                time.sleep(.02)
            self.assertTrue(ready.exists())
            plan = json.loads(self.command('plan', '--budget-gib', '0').stdout)
            self.assertTrue(any(row['path'] == str(target) and row['reason'] == 'building' for row in plan['entries']))
            proc.communicate(timeout=5)
            self.assertEqual(proc.returncode, 0)
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.communicate()

    def test_corrupt_metadata_and_embedded_source_are_never_deleted(self):
        target = self.path()
        self.build()
        self.git('worktree', 'add', '-b', 'embedded', str(target / 'embedded-source'))
        plan = json.loads(self.command('prune', '--apply', '--budget-gib', '0').stdout)
        self.assertTrue(target.exists())
        self.assertTrue(any(row['reason'] == 'source-worktree' for row in plan['entries']))
        (target / '.shifu-cache.json').write_text('{broken')
        self.command('prune', '--apply', '--budget-gib', '0')
        self.assertTrue((target / 'artifact').exists())
        self.assertTrue((target / 'embedded-source/source.txt').exists())
        (target / '.shifu-cache.json').write_text('{}')
        failed = self.command('run', '--kind', 'cli', '--', sys.executable, '-c', WRITE, ok=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn('build-cache:', failed.stderr)
        self.assertNotIn('Traceback', failed.stderr)

    def test_all_desktop_intermediate_paths_stay_inside_the_owned_cache(self):
        code = '''import os,pathlib
root=pathlib.Path(os.environ['CARGO_TARGET_DIR'])
for key in ['SHIFU_REMOTE_BUILD_DIR','SHIFU_CRYPTO_BUILD_DIR','SHIFU_REMOTE_TOOLS_DIR','SHIFU_BUILD_STAGE_ROOT']:
 assert pathlib.Path(os.environ[key]).is_relative_to(root), key
'''
        self.command('run', '--kind', 'desktop', '--', sys.executable, '-c', code)

    def test_timeout_stops_the_command_and_releases_its_build_lock(self):
        out = self.command('run', '--kind', 'cli', '--timeout-seconds', '.2', '--', sys.executable, '-c', 'import time; time.sleep(10)', ok=False)
        self.assertEqual(out.returncode, 124, out.stderr)
        self.build()
        self.assertTrue((self.path() / 'artifact').exists())

    def test_idle_expiry_is_independent_of_budget(self):
        self.build()
        target = self.path()
        meta = target / '.shifu-cache.json'
        data = json.loads(meta.read_text())
        data['last_used'] = time.time() - 4 * 86400
        meta.write_text(json.dumps(data))
        report = json.loads(self.command('prune', '--apply').stdout)
        self.assertFalse(target.exists())
        self.assertTrue(any(row['reason'] == 'expired' for row in report['entries']))

    def test_unmanaged_directory_is_not_adopted_and_target_override_is_rejected(self):
        target = self.path()
        target.mkdir(parents=True)
        (target / 'draft.txt').write_text('keep')
        failed = self.command('run', '--kind', 'cli', '--', sys.executable, '-c', WRITE, ok=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertEqual((target / 'draft.txt').read_text(), 'keep')
        failed = self.command('run', '--kind', 'remote', '--', 'cargo', 'build', '--target-dir', '/elsewhere', ok=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertFalse(self.path('remote').exists())

    def test_failed_build_preserves_its_diagnostics_and_reclaims_older_cache(self):
        self.build()
        old = self.path()
        out = self.command('run', '--kind', 'remote', '--budget-gib', '0', '--', sys.executable, '-c', 'raise SystemExit(7)', ok=False)
        self.assertEqual(out.returncode, 7)
        self.assertFalse(old.exists())
        failed_cache = self.path('remote')
        self.assertTrue(failed_cache.exists())
        data = json.loads((failed_cache / '.shifu-cache.json').read_text())
        self.assertEqual(data['last_exit'], 7)

    def test_default_budget_retires_old_worktree_caches_under_pressure(self):
        self.build()
        old = self.path()
        self.build('remote')
        # The du boundary reports 5 GiB per cache without allocating 10 GiB in
        # the fixture. Ownership, Git, locks, and deletion use the real paths.
        tools = self.root / 'fake-size-tools'
        tools.mkdir()
        du = tools / 'du'
        du.write_text('#!/bin/sh\nfor last; do :; done\nprintf "5242880\\t%s\\n" "$last"\n')
        du.chmod(0o755)
        out = subprocess.run([sys.executable, str(SCRIPT), 'prune', '--apply'],
                             cwd=self.root, env={**os.environ, 'PATH': str(tools) + os.pathsep + os.environ['PATH']},
                             capture_output=True, text=True, timeout=15)
        self.assertEqual(out.returncode, 0, out.stderr)
        report = json.loads(out.stdout)
        self.assertEqual(report['removed'], [str(old)])
        self.assertFalse(old.exists())
        self.assertTrue(self.path('remote').exists())
        self.assertEqual(report['over_budget_bytes'], 0)

    def test_crypto_build_and_tool_paths_follow_the_managed_environment(self):
        script = SCRIPT.with_name('build-remote-crypto.sh')
        scripts = self.root / 'scripts'
        scripts.mkdir()
        shutil.copy2(script, scripts / script.name)
        tools = self.root / 'fake-tools'
        tools.mkdir()
        cargo = tools / 'cargo'
        cargo.write_text('#!/bin/sh\nwhile [ "$#" -gt 0 ]; do if [ "$1" = --target-dir ]; then shift; target=$1; fi; shift; done\nmkdir -p "$target/wasm32-unknown-unknown/release"\nprintf wasm > "$target/wasm32-unknown-unknown/release/shifu_channel.wasm"\n')
        cargo.chmod(0o755)
        rustup = tools / 'rustup'
        rustup.write_text('#!/bin/sh\nexit 0\n')
        rustup.chmod(0o755)
        owned = self.root / 'managed-desktop'
        bindgen = owned / 'tools/bin/wasm-bindgen'
        bindgen.parent.mkdir(parents=True)
        bindgen.write_text('#!/bin/sh\nfor last; do :; done\ntest -f "$last"\n')
        bindgen.chmod(0o755)
        env = {**os.environ, 'PATH': str(tools) + os.pathsep + os.environ['PATH'],
               'SHIFU_REMOTE_TOOLS_DIR': str(owned / 'tools'), 'SHIFU_CRYPTO_BUILD_DIR': str(owned / 'crypto')}
        out = subprocess.run(['bash', str(scripts / script.name)], cwd=self.root, env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertTrue((owned / 'crypto/wasm32-unknown-unknown/release/shifu_channel.wasm').exists())
        self.assertFalse((self.root / 'remote/crypto/target').exists())

    def test_successful_packaging_does_not_accumulate_staging_directories(self):
        scripts = self.root / 'scripts'
        scripts.mkdir()
        shutil.copy2(PREPARE, scripts / PREPARE.name)
        tools = self.root / 'fake-tools'
        tools.mkdir()
        for tool, body in {
            'npm': '#!/bin/sh\nexit 0\n',
            'cargo': '#!/bin/sh\nwhile [ "$#" -gt 0 ]; do if [ "$1" = --target-dir ]; then shift; target=$1; fi; shift; done\nmkdir -p "$target/release"\nprintf program > "$target/release/shifu-remote"\n',
        }.items():
            path = tools / tool
            path.write_text(body)
            path.chmod(0o755)
        web = self.root / 'remote/web-dist'
        web.mkdir(parents=True)
        (web / 'index.html').write_text('new web')
        bundle = self.root / 'app/src-tauri/remote-bundle/web'
        bundle.mkdir(parents=True)
        (bundle / 'index.html').write_text('previous web')
        env = {**os.environ, 'PATH': str(tools) + os.pathsep + os.environ['PATH']}
        for _ in range(2):
            out = subprocess.run([sys.executable, str(SCRIPT), 'run', '--kind', 'desktop', '--', 'bash', str(scripts / PREPARE.name)], cwd=self.root, env=env, capture_output=True, text=True, timeout=15)
            self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(list((self.path('desktop') / 'staging').iterdir()), [])
        self.assertFalse((self.root / 'remote/target').exists())
        self.assertFalse((self.root / 'target/remote-bundle-staging').exists())
        self.assertEqual((bundle / 'index.html').read_text(), 'new web')

    def test_unmanaged_desktop_packaging_stops_before_building(self):
        env = {key: value for key, value in os.environ.items() if key != 'CARGO_TARGET_DIR'}
        out = subprocess.run(['bash', str(PREPARE)], cwd=self.root, env=env, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(out.returncode, 0)
        self.assertIn('build:desktop', out.stderr)
        self.assertFalse((self.root / 'remote/target').exists())

    def test_desktop_npm_entry_passes_build_arguments_and_managed_target(self):
        scripts = self.root / 'scripts'
        scripts.mkdir()
        shutil.copy2(SCRIPT, scripts / SCRIPT.name)
        app = self.root / 'app'
        tool = app / 'node_modules/.bin/tauri'
        tool.parent.mkdir(parents=True)
        tool.write_text(f'#!{sys.executable}\nimport json,os,sys\nprint(json.dumps({{"target":os.environ["CARGO_TARGET_DIR"],"args":sys.argv[1:]}}))\n')
        tool.chmod(0o755)
        package = json.loads((SCRIPT.parents[1] / 'app/package.json').read_text())
        (app / 'package.json').write_text(json.dumps({'scripts': {'build:desktop': package['scripts']['build:desktop']}}))
        out = subprocess.run(['npm', '--prefix', str(app), 'run', 'build:desktop', '--', '--debug'], cwd=self.root, capture_output=True, text=True, timeout=20)
        self.assertEqual(out.returncode, 0, out.stderr)
        value = next(json.loads(line) for line in out.stdout.splitlines() if line.startswith('{"target"'))
        self.assertEqual(value['target'], str(self.path('desktop')))
        self.assertEqual(value['args'], ['build', '--debug'])


if __name__ == '__main__':
    unittest.main()
