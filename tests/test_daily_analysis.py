import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("daily_analysis", Path(__file__).resolve().parents[1] / "scripts/daily-analysis.py")
daily = importlib.util.module_from_spec(spec)
spec.loader.exec_module(daily)


class DailyAnalysisTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.reports = self.root / "reports"
        self.reports.mkdir()
        self.shifu = self.root / "shifu"
        self.shifu.write_text(f"#!{sys.executable}\n" + '''import json,sys
from pathlib import Path
root=Path(__file__).parent
if sys.argv[1]=='status':
 print((root/'conversations.json').read_text())
else:
 with (root/'tells.jsonl').open('a') as f: f.write(json.dumps(sys.argv[1:])+'\\n')
 sys.exit(int((root/'tell-rc').read_text()) if (root/'tell-rc').exists() else 0)
''')
        self.shifu.chmod(0o755)
        self.args = SimpleNamespace(reports=self.reports, first_day=daily.date(2026, 11, 14),
            through=None, task="每日分析", role_dir=self.root / "role", shifu=self.shifu,
            state=self.root / "state.json", dry_run=False)
        self.now = daily.datetime(2026, 11, 17, 10, 0, tzinfo=daily.TZ)
        self.report("2026-11-14")
        self.conversations("idle")
        self.notifier = patch.object(daily, "notify")
        self.notice = self.notifier.start()
        self.addCleanup(self.notifier.stop)

    def conversations(self, status, extra=None):
        rows = [{"name": "codex-daily", "task": "每日分析", "cwd": str(self.args.role_dir), "status": status}]
        if extra:
            rows.extend(extra)
        (self.root / "conversations.json").write_text(json.dumps(rows))

    def report(self, day):
        (self.reports / f"{day}.md").write_text(f"# {day}\nProject progress\n")

    def check(self, minutes=0):
        return daily.run_once(self.args, self.now + daily.timedelta(minutes=minutes))

    def tells(self):
        path = self.root / "tells.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_oldest_gap_then_next_date_only_after_report(self):
        event = self.check()
        self.assertEqual((event["action"], event["day"]), ("sent", "2026-11-15"))
        self.assertIn("开始分析 2026-11-15", self.tells()[0][-1])
        self.assertEqual(self.check(5)["action"], "cooldown")
        self.conversations("running")
        self.report("2026-11-15")
        self.assertEqual(self.check(10)["action"], "busy")
        self.conversations("needs_reply")
        self.assertEqual(self.check(15)["day"], "2026-11-16")
        self.report("2026-11-16")
        self.assertEqual(self.check(20)["action"], "up_to_date")
        self.assertEqual(len(self.tells()), 2)

    def test_not_ready_retries_later_without_counting_a_send(self):
        for status in ("starting", "needs_approval", "exited"):
            self.conversations(status)
            self.assertEqual(self.check()["action"], "waiting")
        self.assertEqual(self.tells(), [])
        self.conversations("idle")
        self.assertEqual(self.check(5)["attempt"], 1)

    def test_other_active_agent_prevents_parallel_analysis(self):
        self.conversations("idle", [{"name": "other-daily", "task": "每日分析", "cwd": str(self.args.role_dir), "status": "running"}])
        self.assertEqual(self.check()["action"], "busy")
        self.assertEqual(self.tells(), [])

    def test_same_name_in_another_directory_is_not_selected(self):
        self.conversations("starting", [{"name": "wrong", "task": "每日分析", "cwd": "/other", "status": "idle"}])
        self.assertEqual(self.check()["action"], "waiting")

    def test_morning_cutoff_and_explicit_backfill(self):
        self.report("2026-11-15")
        self.now = self.now.replace(hour=9, minute=9)
        self.assertEqual(self.check()["action"], "up_to_date")
        self.assertEqual(self.check(1)["day"], "2026-11-16")

    def test_empty_report_is_not_complete_and_dry_run_has_no_writes(self):
        (self.reports / "2026-11-15.md").touch()
        self.args.dry_run = True
        event = self.check()
        self.assertEqual((event["action"], event["day"]), ("would_send", "2026-11-15"))
        self.assertFalse(self.args.state.exists())
        self.assertEqual(self.tells(), [])
        self.notice.assert_not_called()

    def test_missing_report_retries_at_most_three_times_then_allows_next_date(self):
        for minute in (0, 30, 60):
            self.assertEqual(self.check(minute)["day"], "2026-11-15")
        self.assertEqual(self.check(90)["day"], "2026-11-16")
        self.check(120)
        self.check(150)
        self.assertEqual(self.check(180)["action"], "blocked")
        self.assertEqual(len(self.tells()), 6)

    def test_unknown_delivery_is_not_repeated(self):
        (self.root / "tell-rc").write_text("1")
        self.assertEqual(self.check()["action"], "blocked")
        self.assertEqual(self.check(40)["action"], "blocked")
        self.assertEqual(len(self.tells()), 1)

    def test_interrupted_delivery_is_not_repeated(self):
        self.args.state.write_text(json.dumps({"days": {"2026-11-15": {"outcome": "sending", "attempts": 1}}}))
        self.assertEqual(self.check()["action"], "blocked")
        self.assertEqual(self.tells(), [])

    def test_explicit_through_backfills_yesterday_before_cutoff(self):
        self.report("2026-11-15")
        self.now = self.now.replace(hour=7)
        self.args.through = daily.date(2026, 11, 16)
        self.assertEqual(self.check()["day"], "2026-11-16")


if __name__ == "__main__":
    unittest.main()
