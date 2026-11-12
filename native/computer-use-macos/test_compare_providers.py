import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


def load():
    spec = importlib.util.spec_from_file_location('compare_providers', Path(__file__).with_name('compare-providers.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CompareTests(unittest.TestCase):
    def test_provider_order_alternates(self):
        m = load()
        self.assertEqual(m.provider_order(0), ['shifu', 'orca'])
        self.assertEqual(m.provider_order(1), ['orca', 'shifu'])

    def test_nonzero_response_preserves_json_and_tree_bytes(self):
        m = load()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            command = [sys.executable, '-c', 'import json,sys;print(json.dumps({"ok":False,"result":{"snapshot":{"treeText":"中文"}},"error":{"code":"refused"}}));print("detail",file=sys.stderr);sys.exit(3)']
            record = m.run_command(command, root, 'failed', {}, 2)
            self.assertEqual(record['exitCode'], 3)
            self.assertEqual(record['response']['error']['code'], 'refused')
            self.assertEqual(record['treeBytes'], 6)
            self.assertEqual((root/'failed.stderr').read_text().strip(), 'detail')

    def test_timeout_keeps_partial_output(self):
        m = load()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record = m.run_command([sys.executable, '-u', '-c', 'import time;print("partial",flush=True);time.sleep(10)'], root, 'timeout', {}, .1)
            self.assertTrue(record['timedOut'])
            self.assertNotEqual(record['exitCode'], 0)
            self.assertIn('partial', (root/'timeout.stdout').read_text())

    def test_same_sequence_targets_pid_without_foreground_flags(self):
        m = load()
        calls = []
        def fake(command, root, name, env, timeout):
            calls.append(command)
            method = command[2]
            if method == 'list-windows':
                response = {'ok':True,'result':{'windows':[{'id':42,'title':'Shifu Computer Use 测试窗口'}]}}
            else:
                text = '0 window\n1 button 增加计数\n2 text field (settable) 测试输入框\n3 text 点击次数：' + ('0' if name == 'observe-before' else '1')
                if name == 'observe-value': text += '\n4 text 中文对照'
                response = {'ok':True,'result':{'snapshot':{'treeText':text,'window':{'id':42}}}}
            return {'response':response,'exitCode':0,'timedOut':False,'treeBytes':1,'elapsedMs':1}
        for provider in ['shifu','orca']:
            calls.clear()
            result = m.run_sequence(provider, '/absolute/cli', 123, Path('/unused'), {}, 'session', fake)
            self.assertEqual(result['status'], 'passed')
            self.assertEqual([c[2] for c in calls], ['list-windows','get-app-state','click','get-app-state','set-value','get-app-state'])
            self.assertTrue(all(c[c.index('--app')+1] == 'pid:123' for c in calls))
            self.assertTrue(all('--restore-window' not in c for c in calls))
            self.assertNotIn('--session', calls[0])
            self.assertNotIn('--no-screenshot', calls[0])
            self.assertTrue(all('--no-screenshot' in c and '--session' in c for c in calls[1:]))
            if provider == 'orca':
                self.assertTrue(all(c[c.index('--environment') + 1] == 'local' for c in calls))
            self.assertEqual(result['fullObservationTreeBytes'],3)
            self.assertEqual(result['actionCount'],2)


if __name__ == '__main__': unittest.main()
