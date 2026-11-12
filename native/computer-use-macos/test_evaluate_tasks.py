"""Acceptance checks for honest task evaluation; no GUI or model is started."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


def load_runner():
    spec = importlib.util.spec_from_file_location('shifu_task_evaluator', Path(__file__).with_name('evaluate-tasks.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class EvaluationTests(unittest.TestCase):
    def test_counts_complete_tasks_without_counting_mechanisms_as_tasks(self):
        runner = load_runner()
        report = runner.summarize([
            {'tasks': [{'taskId': 'finder_rename', 'status': 'failed'}, {'taskId': 'browser_target_and_restore', 'status': 'not_run'}], 'mechanisms': {'button': True, 'text': True}},
            {'tasks': [{'taskId': 'finder_rename', 'status': 'passed'}, {'taskId': 'browser_target_and_restore', 'status': 'passed'}]},
        ])
        self.assertEqual(report['tasks']['finder_rename'], {'passed': 1, 'failed': 1, 'not_run': 0})
        self.assertEqual(report['tasks']['browser_target_and_restore'], {'passed': 1, 'failed': 0, 'not_run': 1})
        self.assertEqual(len(report['tasks']), 2)

    def test_nonzero_child_retains_its_report_and_error_exit(self):
        runner = load_runner()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result_dir = root / 'result'
            child = root / 'child.py'
            child.write_text('import json,sys\nfrom pathlib import Path\np=Path(sys.argv[1]);p.mkdir()\n(p/"result.json").write_text(json.dumps({"tasks":[{"taskId":"finder_rename","status":"failed","errorCode":"window_not_focused"}]}))\nraise SystemExit(3)\n')
            record = runner.run_suite([sys.executable, str(child), str(result_dir)], result_dir, 3)
            self.assertEqual(record['process']['exitCode'], 3)
            self.assertFalse(record['process']['timedOut'])
            self.assertEqual(record['tasks'][0]['errorCode'], 'window_not_focused')

    def test_timeout_and_missing_report_never_invent_passes(self):
        runner = load_runner()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record = runner.run_suite([sys.executable, '-c', 'import time;time.sleep(10)'], root / 'result', .05)
            self.assertTrue(record['process']['timedOut'])
            self.assertTrue(all(t['status'] != 'passed' for t in record['tasks']))
            self.assertNotEqual(record['process']['exitCode'], 0)


if __name__ == '__main__':
    unittest.main()
