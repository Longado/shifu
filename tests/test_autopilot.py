"""Exercise the real controller with isolated Shifu/Codex command boundaries."""

from datetime import datetime, timedelta, timezone
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/autopilot.py"
spec = importlib.util.spec_from_file_location("shifu_autopilot", SCRIPT)
autopilot = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = autopilot
spec.loader.exec_module(autopilot)


FAKE_CLI = r'''#!/usr/bin/env python3
import json, os, pathlib, sys, time
root = pathlib.Path(os.environ["AUTOPILOT_FIXTURE"])
control = json.loads((root / "control.json").read_text())
def append(name, value):
    with (root / name).open("a") as f: f.write(json.dumps(value, ensure_ascii=False) + "\n")
if pathlib.Path(sys.argv[0]).name == "shifu":
    if sys.argv[1:3] == ["status", "--json"]:
        time.sleep(control.get("status_sleep", 0))
        append("statuses.jsonl", {"at":time.monotonic()})
        if control.get("pause_lead_after_judge") and (root / "judges.jsonl").exists() and not (root / "lead-pause-injected").exists():
            with (root / "transcripts/rollout-session-lead.jsonl").open("a") as f:
                f.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"暂停，不要继续。"}]}}) + "\n")
            (root / "lead-pause-injected").touch()
        rows = []
        for path in (root / "home/agents").glob("*.json"):
            d = json.loads(path.read_text())
            rows.append({"name":d["name"], "kind":d["kind"], "cwd":d["cwd"], "status":d["status"], "since":d["status_since"], "last_message":"only a 300 character summary"})
        rows.append({"name":"not-whitelisted", "status":"needs_reply"})
        print(json.dumps(rows))
    elif sys.argv[1] == "tell":
        append("sends.jsonl", {"name":sys.argv[2], "argv":sys.argv[3:]})
        if "--queue" in sys.argv:
            record = json.loads((root / "home/agents" / (sys.argv[2] + ".json")).read_text())
            ident=sys.argv[sys.argv.index("--id")+1]
            text="（%s转来）%s" % (sys.argv[sys.argv.index("--from")+1],sys.argv[-1])
            path=root / "home/inbox" / sys.argv[2] / (ident+".json");path.parent.mkdir(parents=True,exist_ok=True)
            phase=control.get("queue_phase","queued")
            path.write_text(json.dumps({"id":ident,"agent":sys.argv[2],"phase":phase,"session_id":record["session_id"],"cwd":record["cwd"],"text":text}))
            if phase == "received":
                with (root / "transcripts" / ("rollout-" + record["session_id"] + ".jsonl")).open("a") as f:
                    f.write(json.dumps({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}})+"\n")
            print(json.dumps({"id":ident,"agent":sys.argv[2],"phase":phase}));sys.exit(0)
        if control.get("tell_echo"):
            record = json.loads((root / "home/agents" / (sys.argv[2] + ".json")).read_text())
            text = "（%s转来）%s" % (sys.argv[sys.argv.index("--from") + 1], sys.argv[-1])
            with (root / "transcripts" / ("rollout-" + record["session_id"] + ".jsonl")).open("a") as f:
                f.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":text}]}}) + "\n")
                if control.get("tell_final") and len((root / "sends.jsonl").read_text().splitlines()) == 1:
                    f.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"assistant", "phase":"final_answer", "content":[{"type":"output_text", "text":"A fast new result arrived before the CLI receipt."}]}}) + "\n")
        delay = control.get("tell_sleep_by_agent", {}).get(sys.argv[2], control.get("tell_sleep", 0))
        if delay: time.sleep(delay)
        while sys.argv[2] in control.get("tell_hold_agents", []) and not (root / ("release-" + sys.argv[2])).exists(): time.sleep(.02)
        if control.get("tell_unknown"):
            print("submission outcome unavailable", file=sys.stderr); sys.exit(7)
        if control.get("tell_no_ack"):
            print("command returned without a target receipt"); sys.exit(0)
        print("已发给「fixture」[" + sys.argv[2] + "]")
    else: sys.exit(9)
else:
    if "mcp" in sys.argv and sys.argv[sys.argv.index("mcp"):sys.argv.index("mcp")+3] == ["mcp", "list", "--json"]:
        append("inventories.jsonl", sys.argv[1:])
        print('[{"name":"fixture-server"}]')
    else:
        prompt = sys.stdin.read()
        context = json.loads(prompt[prompt.index('{"controller_scope"'):])
        schema = json.loads(pathlib.Path(sys.argv[sys.argv.index("--output-schema") + 1]).read_text())
        append("judges.jsonl", {"at":time.monotonic(), "argv":sys.argv[1:], "context":context, "schema":schema, "source_notice":prompt[:prompt.index('{"controller_scope"')], "shifu_id":os.environ.get("SHIFU_ID"), "codex_home":os.environ.get("CODEX_HOME")})
        (root / "judge.pid").write_text(str(os.getpid()))
        (root / ("judge-" + context["controller_scope"]["agent"] + ".pid")).write_text(str(os.getpid()))
        delay = control.get("judge_sleep_by_agent", {}).get(context["controller_scope"]["agent"], control.get("judge_sleep", 0))
        if delay: time.sleep(delay)
        while context["controller_scope"]["agent"] in control.get("judge_hold_agents", []) and not (root / ("release-" + context["controller_scope"]["agent"])).exists(): time.sleep(.02)
        if control.get("judge_fail"): sys.exit(4)
        if control.get("judge_status"):
            p = root / "home/agents/a.json"; d = json.loads(p.read_text()); d["status"] = control["judge_status"]; p.write_text(json.dumps(d))
        if control.get("judge_stop"): (root / "STOP").touch()
        if control.get("judge_artifact"): (root / "project/delivery.md").write_text("changed during judgment")
        if control.get("judge_user"):
            with (root / "transcripts/rollout-session-a.jsonl").open("a") as f:
                f.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"already has another task"}]}}) + "\n")
                if control.get("judge_user_final"):
                    f.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"assistant", "phase":"final_answer", "content":[{"type":"output_text", "text":"好的，已暂停。"}]}}) + "\n")
        action = control.get("action", "follow_up")
        decision = {"action":action, "reason":"The full reply and actual delivery leave one scoped check.", "next_step":"Check the delivered behavior using the existing focused test.", "evidence":[context["messages"][-1]["source_id"], context["artifacts"][0]["source_id"]], "target_paths":[str(root / "project/test.txt")] if action == "follow_up" else []}
        if control.get("outside_path"): decision["target_paths"] = [str(root / "outside.txt")]
        if control.get("fake_evidence"): decision["evidence"] = ["artifact:not-read"]
        if control.get("empty_evidence"): decision["evidence"] = []
        if control.get("many_evidence"): decision["evidence"] *= 9
        if control.get("credential_answer"):
            decision.update(api_key="sk-fixture-secret-12345678", password="fixture-private-password", reason="Bearer fixture-private-token-12345; password=fixture-inline-secret")
        answer = pathlib.Path(sys.argv[sys.argv.index("--output-last-message") + 1]); answer.write_text(json.dumps(decision))
        print(json.dumps({"type":"item.completed", "item":{"type":"agent_message", "text":json.dumps(decision)}}))
        if control.get("usage"): print(json.dumps({"type":"turn.completed", "usage":control["usage"]}))
'''


class AutopilotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="shifu-autopilot-")
        self.root = Path(self.temp.name).resolve()
        for path in ("home/agents", "project", "transcripts", "run", "bin"):
            (self.root / path).mkdir(parents=True)
        for name in ("shifu", "codex"):
            path = self.root / "bin" / name
            path.write_text(FAKE_CLI)
            path.chmod(0o755)
        self.control = {}
        self.save_control()
        self.record = {"name":"a", "kind":"codex", "session_id":"session-a", "cwd":str(self.root / "project"), "status":"needs_reply", "status_since":12, "created_at":1, "pane_id":"%1"}
        self.save_record()
        self.body = "完整回复。" * 400 + "TAIL_AFTER_300: remains unverified; do not install the formal app."
        self.transcript = self.root / "transcripts/rollout-session-a.jsonl"
        self.transcript.write_text("")
        self.append({"type":"session_meta", "payload":{"id":"session-a"}})
        self.append_message("user", "Finish the smallest in-scope change.")
        self.append_message("assistant", "intermediate commentary", "commentary")
        self.append_message("assistant", self.body, "final_answer")
        self.append({"type":"event_msg", "payload":{"type":"task_complete", "last_agent_message":self.body}})
        self.artifact = self.root / "project/delivery.md"
        self.artifact.write_text("Actual delivery content. " * 100)
        self.raw = {
            "run_id":"test-run",
            "deadline":(datetime.now(timezone.utc) + timedelta(hours=1)).isoformat(),
            "shifu_home":str(self.root / "home"), "shifu_socket":"fixture-socket",
            "shifu_bin":str(self.root / "bin/shifu"), "codex_bin":str(self.root / "bin/codex"),
            "transcripts_dir":str(self.root / "transcripts"),
            "state_path":str(self.root / "run/state.json"), "log_path":str(self.root / "run/events.jsonl"), "stop_path":str(self.root / "STOP"),
            "command_timeout_seconds":5, "judge_timeout_seconds":10,
            "agents":[{"name":"a", "session_id":"session-a", "authorized_user":autopilot.latest_user_turn(self.transcript, "session-a"), "cwd":str(self.root / "project"), "goal":"Complete the focused delivery check.", "allowed_paths":[str(self.root / "project")], "preserve":["Do not install, restart or replace the formal app."], "artifacts":[str(self.artifact)]}],
        }
        self.config_path = self.root / "config.json"
        self.env = dict(os.environ, AUTOPILOT_FIXTURE=str(self.root), SHIFU_ID="hosting-agent-must-not-leak")

    def tearDown(self):
        if getattr(self, "background", False):
            self.run_control("--stop")
        self.temp.cleanup()

    def save_control(self):
        (self.root / "control.json").write_text(json.dumps(self.control))

    def save_record(self):
        (self.root / "home/agents/a.json").write_text(json.dumps(self.record))

    def append(self, value):
        with self.transcript.open("a") as f:
            f.write(json.dumps(value, ensure_ascii=False) + "\n")

    def append_message(self, role, text, phase=None):
        self.append({"type":"response_item", "payload":{"type":"message", "role":role, "phase":phase, "content":[{"type":"output_text" if role == "assistant" else "input_text", "text":text}]}})

    def run_tick(self, *args, ok=True):
        self.config_path.write_text(json.dumps(self.raw))
        result = subprocess.run([sys.executable, str(SCRIPT), "--config", str(self.config_path), "--once", *args], capture_output=True, text=True, env=self.env, timeout=15)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
        return result, [json.loads(line) for line in result.stdout.splitlines()]

    def rows(self, name):
        path = self.root / name
        text = path.read_text() if path.exists() else ""
        complete = text.splitlines()
        if text and not text.endswith("\n"):
            complete.pop()
        return [json.loads(line) for line in complete]

    def run_control(self, mode):
        if mode == "--start" or not self.config_path.exists():
            self.config_path.write_text(json.dumps(self.raw))
        result = subprocess.run([sys.executable, str(SCRIPT), "--config", str(self.config_path), mode], capture_output=True, text=True, env=self.env, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        if mode == "--start":
            self.background = True
        return json.loads(result.stdout)

    def wait_for(self, check, timeout=5):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            if check():
                return
            time.sleep(0.02)
        self.fail("background condition did not arrive")

    def background_event(self, name):
        return self.run_control("--status")["agents"].get(name) or {}

    def enable_supervisor(self, status="needs_reply"):
        record = dict(self.record, name="lead", session_id="session-lead", pane_id="%3", status=status)
        (self.root / "home/agents/lead.json").write_text(json.dumps(record))
        path = self.root / "transcripts/rollout-session-lead.jsonl"
        path.write_text(json.dumps({"type": "session_meta", "payload": {"id": "session-lead"}}) + "\n" +
                        json.dumps({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Supervise this bounded run."}]}}) + "\n")
        self.raw["supervisor"] = {"name": "lead", "session_id": "session-lead", "cwd": record["cwd"],
                                  "authorized_user": autopilot.latest_user_turn(path, "session-lead")}
        return path

    def test_linked_screenshot_is_metadata_not_utf8_or_visual_evidence(self):
        image = self.root / "project/screenshot.png"
        image.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x00" * 100000)
        self.append_message("assistant", "已有截图：[画面](%s)。" % image, "final_answer")
        self.run_tick("--dry-run")
        context = self.rows("judges.jsonl")[0]["context"]
        artifact = next(a for a in context["artifacts"] if a["path"] == str(image))
        self.assertEqual(artifact["review"], "metadata_only")
        self.assertEqual(artifact["bytes"], image.stat().st_size)
        self.assertIsNone(artifact["sha256"])
        self.assertIn("content/visual inspection is still required", artifact["text"])
        controller = autopilot.Autopilot(autopilot.Config(self.raw), True)
        first = controller.context(controller.config.agents[0])
        image.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x01" * 100000)
        second = controller.context(controller.config.agents[0])
        self.assertNotEqual(first["hash"], second["hash"])

    def test_read_only_delivery_root_is_read_but_never_authorizes_writes(self):
        delivery = self.root / "outside.txt"
        delivery.write_text("Candidate receipt outside the source worktree.")
        self.raw["agents"][0]["read_paths"] = [str(delivery)]
        self.append_message("assistant", "候选回执：[文件](%s)。" % delivery, "final_answer")
        self.run_tick("--dry-run")
        context = self.rows("judges.jsonl")[0]["context"]
        self.assertEqual(context["artifacts"][-1]["text"], delivery.read_text())
        self.assertEqual(context["controller_scope"]["read_paths"], [str(delivery)])
        self.control["outside_path"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertIn("outside allowed_paths", events[-1]["reason"])
        self.assertFalse(self.rows("sends.jsonl"))

    def test_blocked_run_notifies_pinned_supervisor_once_and_exits(self):
        self.enable_supervisor()
        self.control.update(action="needs_user", tell_echo=True); self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        status = self.run_control("--status")
        self.assertEqual(status["activity_status"], "blocked")
        self.assertEqual(status["supervisor"]["phase"], "sent")
        self.assertEqual([s["name"] for s in self.rows("sends.jsonl")], ["lead"])
        self.assertIn(str(self.config_path), self.rows("sends.jsonl")[0]["argv"][-1])
        self.run_tick()
        self.assertEqual(len(self.rows("sends.jsonl")), 1)
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertFalse(state["supervisor"]["user_guard"]["stopped"])

    def test_busy_supervisor_queues_report_then_wakes_when_idle(self):
        self.enable_supervisor("running")
        self.raw["poll_seconds"] = 1
        self.control["action"] = "needs_user"; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: self.run_control("--status")["activity_status"] == "waiting_supervisor")
        self.assertFalse(self.rows("sends.jsonl"))
        path = self.root / "home/agents/lead.json"
        record = json.loads(path.read_text()); record["status"] = "needs_reply"; path.write_text(json.dumps(record))
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        self.assertEqual([s["name"] for s in self.rows("sends.jsonl")], ["lead"])

    def test_supervisor_identity_change_never_retargets_notification(self):
        self.enable_supervisor()
        path = self.root / "home/agents/lead.json"
        record = json.loads(path.read_text()); record["session_id"] = "replacement"; path.write_text(json.dumps(record))
        self.control["action"] = "needs_user"; self.save_control()
        self.run_tick()
        state = json.loads((self.root / "run/state.json").read_text())
        notice = next(iter(state["supervisor"]["replies"].values()))
        self.assertEqual(notice["phase"], "rejected")
        self.assertIn("identity changed", notice["error"])
        self.assertFalse(self.rows("sends.jsonl"))

    def test_new_human_input_to_supervisor_revokes_run_before_notification(self):
        path = self.enable_supervisor("running")
        self.control["action"] = "needs_user"; self.save_control()
        self.run_tick()
        with path.open("a") as stream:
            stream.write(json.dumps({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "暂停，不要继续。"}]}}) + "\n")
        self.run_tick()
        self.assertFalse(self.rows("sends.jsonl"))
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertTrue(state["supervisor"]["user_guard"]["stopped"])

    def test_unknown_supervisor_notification_is_never_automatically_retried(self):
        self.enable_supervisor()
        self.control.update(action="needs_user", tell_unknown=True); self.save_control()
        self.run_tick()
        self.run_tick()
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertEqual(next(iter(state["supervisor"]["replies"].values()))["phase"], "unknown")
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_human_pause_to_lead_cancels_inflight_work_before_any_notification(self):
        path = self.enable_supervisor("running")
        self.raw["judge_timeout_seconds"] = 30
        self.control["judge_hold_agents"] = ["a"]; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: (self.root / "judge-a.pid").exists())
        with path.open("a") as stream:
            stream.write(json.dumps({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "暂停。"}]}}) + "\n")
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        self.assertFalse(self.rows("sends.jsonl"))
        with self.assertRaises(ProcessLookupError):
            os.kill(int((self.root / "judge-a.pid").read_text()), 0)

    def test_completed_background_stops_early_and_elapsed_time_is_frozen(self):
        self.control["action"] = "done"; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        first = self.run_control("--status")
        self.assertEqual(first["activity_status"], "completed")
        self.assertLess(first["elapsed_seconds"], 10)
        self.assertIsNotNone(first["last_activity_at"])
        self.assertEqual(first["elapsed_seconds"], self.run_control("--status")["elapsed_seconds"])

    def test_lead_pause_during_dispatch_snapshot_prevents_target_send(self):
        self.enable_supervisor("running")
        self.control["pause_lead_after_judge"] = True; self.save_control()
        self.run_tick()
        self.assertTrue((self.root / "lead-pause-injected").exists())
        self.assertFalse(self.rows("sends.jsonl"))

    def test_lead_pause_while_target_receipt_pending_stops_child_and_keeps_unknown(self):
        lead = self.enable_supervisor("running")
        self.raw["command_timeout_seconds"] = 30
        self.control["tell_hold_agents"] = ["a"]; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: len(self.rows("sends.jsonl")) == 1)
        with lead.open("a") as f:
            f.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"暂停。"}]}}) + "\n")
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        state = json.loads((self.root / "run/state.json").read_text())
        pending = state["agents"]["a"]["uncertain_send"]
        self.assertEqual(state["agents"]["a"]["replies"][pending]["phase"], "unknown")
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_unknown_lead_notification_blocks_new_reports_while_other_target_finishes(self):
        self.enable_supervisor()
        self.raw["judge_timeout_seconds"] = 30
        self.raw["agents"].append(dict(self.raw["agents"][0], name="b", session_id="session-b"))
        (self.root / "home/agents/b.json").write_text(json.dumps(dict(self.record, name="b", session_id="session-b", pane_id="%2")))
        (self.root / "transcripts/rollout-session-b.jsonl").write_text(self.transcript.read_text().replace("session-a", "session-b"))
        self.control.update(action="needs_user", tell_unknown=True, judge_hold_agents=["b"]); self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: self.run_control("--status")["supervisor"]["phase"] == "unknown")
        self.control["tell_unknown"] = False; self.save_control()
        (self.root / "release-b").touch()
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        state = json.loads((self.root / "run/state.json").read_text())
        lead = state["supervisor"]
        self.assertEqual(lead["replies"][lead["uncertain_send"]]["phase"], "unknown")
        self.assertTrue(any(entry["phase"] == "queued" for entry in lead["replies"].values()))
        self.assertEqual(self.run_control("--status")["supervisor"]["phase"], "unknown")
        self.assertEqual(len(self.rows("sends.jsonl")), 1)
        self.assertEqual(state["agents"]["b"]["replies"].popitem()[1]["phase"], "handled")

    def test_recovered_judging_without_a_live_job_is_failed_not_working(self):
        self.enable_supervisor("running")
        self.control["action"] = "done"; self.save_control()
        self.run_tick()
        path = self.root / "run/state.json"
        state = json.loads(path.read_text())
        entry = next(iter(state["agents"]["a"]["replies"].values())); entry["phase"] = "judging"
        path.write_text(json.dumps(state))
        self.run_control("--start")
        self.wait_for(lambda: self.background_event("a").get("action") == "needs_user")
        status = self.run_control("--status")
        self.assertEqual(status["activity_status"], "waiting_supervisor")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertEqual(next(iter(json.loads(path.read_text())["agents"]["a"]["replies"].values()))["phase"], "judge_failed")

    def test_review_pause_after_send_intent_is_saved_still_prevents_cli_start(self):
        lead = self.enable_supervisor("running")
        with patch.dict(os.environ, self.env):
            controller = autopilot.Autopilot(autopilot.Config(self.raw), quiet=True)
            original_save = controller.save
            injected = False

            def save_then_pause():
                nonlocal injected
                original_save()
                agent = controller.state["agents"].get("a", {})
                if not injected and agent.get("uncertain_send"):
                    injected = True
                    with lead.open("a") as stream:
                        stream.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"暂停，不要发送。"}]}}) + "\n")

            controller.save = save_then_pause
            controller.run(once=True)
        self.assertTrue(injected)
        self.assertFalse(self.rows("sends.jsonl"))
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertTrue(state["supervisor"]["user_guard"]["stopped"])
        self.assertNotIn("uncertain_send", state["agents"]["a"])

    def test_review_pause_during_lead_notification_receipt_stops_without_resend(self):
        lead = self.enable_supervisor()
        self.raw["command_timeout_seconds"] = 30
        self.control.update(action="needs_user", tell_echo=True, tell_hold_agents=["lead"])
        self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: "test-run" in lead.read_text())
        time.sleep(.2)
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertFalse(state["supervisor"]["user_guard"]["stopped"])
        self.assertLess(state["supervisor"]["user_guard"]["cursor"], lead.stat().st_size)
        with lead.open("a") as stream:
            stream.write(json.dumps({"type":"response_item", "payload":{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"暂停。"}]}}) + "\n")
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        state = json.loads((self.root / "run/state.json").read_text())
        supervisor = state["supervisor"]
        self.assertTrue(supervisor["user_guard"]["stopped"])
        self.assertEqual(supervisor["replies"][supervisor["uncertain_send"]]["phase"], "unknown")
        self.run_tick()
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_review_inflight_own_notice_is_only_exempted_after_confirmed_receipt(self):
        lead = self.enable_supervisor()
        self.control.update(action="needs_user", tell_echo=True, tell_hold_agents=["lead"])
        self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: "test-run" in lead.read_text())
        time.sleep(.2)
        state = json.loads((self.root / "run/state.json").read_text())
        supervisor = state["supervisor"]
        self.assertFalse(supervisor["user_guard"]["stopped"])
        self.assertLess(supervisor["user_guard"]["cursor"], lead.stat().st_size)
        (self.root / "release-lead").touch()
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        self.assertEqual(self.run_control("--status")["supervisor"]["phase"], "sent")
        self.run_tick()
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertFalse(state["supervisor"]["user_guard"]["stopped"])
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_concurrent_starts_share_the_one_background_process(self):
        self.record["status"] = "running"
        self.save_record()
        self.config_path.write_text(json.dumps(self.raw))
        argv = [sys.executable, str(SCRIPT), "--config", str(self.config_path), "--start"]
        self.background = True
        launchers = [subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=self.env) for _ in range(6)]
        records = []
        for launcher in launchers:
            output, err = launcher.communicate(timeout=12)
            self.assertEqual(launcher.returncode, 0, err)
            records.append(json.loads(output))
        pid = self.run_control("--status")["pid"]
        self.assertGreater(pid, 0)
        self.assertEqual({row["pid"] for row in records}, {pid})
        self.assertEqual(sum(bool(row.get("already_running")) for row in records), 5)
        self.assertEqual(self.rows("judges.jsonl"), [])
        self.assertEqual(self.rows("sends.jsonl"), [])

    def test_stop_during_startup_waits_for_the_owner_before_stopping(self):
        self.record["status"] = "running"
        self.save_record()
        self.config_path.write_text(json.dumps(self.raw))
        controller = autopilot.Autopilot(autopilot.Config(self.raw))
        command = [sys.executable, str(SCRIPT), "--config", str(self.config_path)]
        self.background = True
        child = stopper = None
        try:
            with autopilot.run_lock(controller.start_lock):
                stopper = subprocess.Popen(command + ["--stop"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env, text=True)
                with self.assertRaises(subprocess.TimeoutExpired):
                    stopper.wait(timeout=.2)
                child = subprocess.Popen(command + ["--quiet"], env=self.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                self.wait_for(lambda: self.run_control("--status")["pid"] == child.pid)
            output, err = stopper.communicate(timeout=8)
            self.assertEqual(stopper.returncode, 0, err)
            self.assertEqual(json.loads(output)["status"], "stopped")
            self.assertTrue(controller.config.stop_path.exists())
            self.assertEqual(child.wait(timeout=3), 0)
        finally:
            for proc in (child, stopper):
                if proc is not None and proc.poll() is None:
                    proc.terminate()
                    proc.wait(timeout=3)

    def test_detached_start_returns_before_judgment_and_stop_reaps_its_child(self):
        self.raw["judge_timeout_seconds"] = 30
        self.control["judge_sleep"] = 20; self.save_control()
        started = time.monotonic()
        result = self.run_control("--start")
        self.assertLess(time.monotonic() - started, 3)
        self.assertEqual(result["status"], "running")
        self.wait_for(lambda: len(self.rows("judges.jsonl")) == 1)
        again = self.run_control("--start")
        self.assertTrue(again["already_running"])
        self.assertEqual(again["pid"], result["pid"])
        stopped = self.run_control("--stop")
        self.assertEqual(stopped["status"], "stopped")
        state = json.loads((self.root / "run/state.json").read_text())
        entry = next(iter(state["agents"]["a"]["replies"].values()))
        self.assertEqual(entry["phase"], "judge_failed")
        self.assertTrue(entry["judgment"]["cancelled"])
        self.assertIsNone(entry["judgment"]["usage"])
        self.assertGreater(entry["judgment"]["elapsed_seconds"], 0)
        self.assertAlmostEqual(stopped["metrics"]["judgment_seconds"], entry["judgment"]["elapsed_seconds"], delta=0.001)
        self.assertEqual(stopped["metrics"], self.run_control("--stop")["metrics"])
        self.assertFalse(self.rows("sends.jsonl"))
        with self.assertRaises(ProcessLookupError):
            os.kill(int((self.root / "judge.pid").read_text()), 0)

    def test_idle_background_does_not_repeat_commands_context_reads_state_or_logs(self):
        self.raw["poll_seconds"] = 1
        self.control["action"] = "done"; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: self.rows("judges.jsonl") and (self.root / "run/events.jsonl").exists())
        status = self.run_control("--status")
        self.assertEqual(status["agents"]["a"]["action"], "done")
        state = self.root / "run/state.json"
        stamp, commands, events = state.stat().st_mtime_ns, len(self.rows("statuses.jsonl")), len(self.rows("run/events.jsonl"))
        time.sleep(2.2)
        status = self.run_control("--status")
        self.assertEqual((state.stat().st_mtime_ns, len(self.rows("statuses.jsonl")), len(self.rows("run/events.jsonl"))), (stamp, commands, events))
        self.assertEqual(status["metrics"]["context_reads"], 1)
        self.assertEqual(status["metrics"]["judgment_attempts"], 1)
        self.assertEqual(len(self.rows("judges.jsonl")), 1)

    def test_new_reply_wakes_background_without_the_sixty_second_probe_delay(self):
        self.raw["poll_seconds"] = 60
        self.control["tell_echo"] = True; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: len(self.rows("sends.jsonl")) == 1)
        started = time.monotonic()
        self.append_message("assistant", "The next scoped result is ready.", "final_answer")
        self.wait_for(lambda: len(self.rows("sends.jsonl")) == 2, timeout=3)
        self.assertLess(time.monotonic() - started, 3)
        self.assertEqual(len(self.rows("judges.jsonl")), 2)

    def test_slow_target_does_not_delay_the_other_targets_dispatch(self):
        self.raw["judge_timeout_seconds"] = 30
        self.raw["agents"].append(dict(self.raw["agents"][0], name="b", session_id="session-b"))
        (self.root / "home/agents/b.json").write_text(json.dumps(dict(self.record, name="b", session_id="session-b", pane_id="%2")))
        (self.root / "transcripts/rollout-session-b.jsonl").write_text(self.transcript.read_text().replace("session-a", "session-b"))
        self.control["judge_hold_agents"] = ["a"]; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: (self.root / "judge-a.pid").exists())
        self.wait_for(lambda: any(row["name"] == "b" for row in self.rows("sends.jsonl")), timeout=8)
        self.assertEqual([row["name"] for row in self.rows("sends.jsonl")], ["b"])
        self.assertEqual({row["context"]["controller_scope"]["agent"] for row in self.rows("judges.jsonl")}, {"a", "b"})

    def test_slow_send_receipt_does_not_block_the_other_target(self):
        self.raw["command_timeout_seconds"] = 30
        self.raw["agents"].append(dict(self.raw["agents"][0], name="b", session_id="session-b"))
        (self.root / "home/agents/b.json").write_text(json.dumps(dict(self.record, name="b", session_id="session-b", pane_id="%2")))
        (self.root / "transcripts/rollout-session-b.jsonl").write_text(self.transcript.read_text().replace("session-a", "session-b"))
        self.control["tell_hold_agents"] = ["a"]; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: self.background_event("b").get("send_state") == "sent", timeout=8)
        self.assertEqual(self.background_event("a").get("send_state"), "sending")
        self.run_control("--stop")
        self.assertEqual(self.background_event("a").get("send_state"), "unknown")
        self.assertEqual(len(self.rows("sends.jsonl")), 2)

    def test_fast_reply_before_send_receipt_is_not_lost_or_mistaken_for_takeover(self):
        self.raw["command_timeout_seconds"] = 4
        self.control.update(tell_echo=True, tell_final=True, tell_sleep=1); self.save_control()
        self.run_control("--start")
        # This is two complete child-process/receipt cycles; arrival-before-receipt is set by the fixture.
        self.wait_for(lambda: len(self.rows("sends.jsonl")) == 2, timeout=15)
        self.wait_for(lambda: self.background_event("a").get("send_state") not in ("sending", "unknown"))
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertFalse(state["agents"]["a"]["user_guard"]["stopped"])
        self.assertEqual(len(self.rows("judges.jsonl")), 2)

    def test_takeover_cancels_only_its_inflight_judgment(self):
        self.raw["judge_timeout_seconds"] = 10
        self.raw["agents"].append(dict(self.raw["agents"][0], name="b", session_id="session-b"))
        (self.root / "home/agents/b.json").write_text(json.dumps(dict(self.record, name="b", session_id="session-b", pane_id="%2")))
        (self.root / "transcripts/rollout-session-b.jsonl").write_text(self.transcript.read_text().replace("session-a", "session-b"))
        self.control.update(judge_sleep_by_agent={"a": 5}, action="done"); self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: (self.root / "judge-a.pid").exists())
        self.append_message("user", "暂停，我来接手。")
        self.append_message("assistant", "已暂停。", "final_answer")
        self.wait_for(lambda: self.background_event("a").get("send_state") == "human_stopped")
        self.wait_for(lambda: self.background_event("b").get("action") == "done")
        def child_gone():
            try:
                os.kill(int((self.root / "judge-a.pid").read_text()), 0)
                return False
            except ProcessLookupError:
                return True
        self.wait_for(child_gone)
        self.assertFalse(self.rows("sends.jsonl"))
        state = json.loads((self.root / "run/state.json").read_text())
        entries = [next(iter(state["agents"][name]["replies"].values())) for name in ("a", "b")]
        self.assertTrue(entries[0]["judgment"]["cancelled"])
        self.assertIsNone(entries[0]["judgment"]["usage"])
        self.assertGreater(entries[0]["judgment"]["elapsed_seconds"], 0)
        self.assertNotIn("cancelled", entries[1]["judgment"])
        self.assertAlmostEqual(state["metrics"]["judgment_seconds"], sum(entry["judgment"]["elapsed_seconds"] for entry in entries), delta=0.002)
        self.assertEqual(state["metrics"], self.run_control("--stop")["metrics"])

    def test_judgment_metrics_keep_usage_separate_from_billing(self):
        usage = {"input_tokens": 1000, "cached_input_tokens": 800, "output_tokens": 30}
        self.control.update(action="done", usage=usage); self.save_control()
        self.run_tick()
        state = json.loads((self.root / "run/state.json").read_text())
        entry = next(iter(state["agents"]["a"]["replies"].values()))
        self.assertEqual(entry["judgment"]["usage"], usage)
        self.assertGreater(entry["judgment"]["elapsed_seconds"], 0)
        self.assertGreater(state["metrics"]["context_bytes"], 0)
        self.assertNotIn("cost", entry["judgment"])
        status = self.run_control("--status")
        self.assertEqual(status["usage"], {"reported_judgments": 1, **usage})
        self.assertNotIn("cancelled", entry["judgment"])
        self.assertAlmostEqual(state["metrics"]["judgment_seconds"], entry["judgment"]["elapsed_seconds"], delta=0.001)
        self.run_tick()
        repeated = json.loads((self.root / "run/state.json").read_text())
        self.assertEqual(state["metrics"]["judgment_seconds"], repeated["metrics"]["judgment_seconds"])
        self.assertEqual(len(self.rows("judges.jsonl")), 1)

    def test_context_cache_invalidates_after_same_size_preserved_mtime_edit(self):
        controller = autopilot.Autopilot(autopilot.Config(self.raw))
        first = controller.context(controller.config.agents[0])
        self.assertIs(controller.context(controller.config.agents[0]), first)
        previous = self.artifact.stat()
        self.artifact.write_text(self.artifact.read_text().replace("Actual", "Edited"))
        os.utime(self.artifact, ns=(previous.st_atime_ns, previous.st_mtime_ns))
        second = controller.context(controller.config.agents[0])
        self.assertNotEqual(first["hash"], second["hash"])
        self.assertTrue(second["artifacts"])
        self.assertEqual(controller.state["metrics"]["artifact_reads"], 2)

    def test_new_replies_reuse_complete_unchanged_artifacts(self):
        controller = autopilot.Autopilot(autopilot.Config(self.raw))
        scope = controller.config.agents[0]
        original_open = Path.open
        reads = []

        def observed_open(path, mode="r", *args, **kwargs):
            if path == self.artifact and mode == "rb":
                reads.append(path)
            return original_open(path, mode, *args, **kwargs)

        with patch.object(Path, "open", observed_open):
            first = controller.context(scope)
            self.append_message("assistant", "A new result using the same delivery.", "final_answer")
            second = controller.context(scope)
        self.assertNotEqual(first["reply_id"], second["reply_id"])
        self.assertEqual(reads, [self.artifact])
        for result in (first, second):
            context = json.loads(result["prompt"][result["prompt"].index('{"controller_scope"'):])
            self.assertEqual(context["artifacts"][0]["text"], self.artifact.read_text())
        self.assertEqual(first["artifacts"], second["artifacts"])
        self.assertEqual(controller.state["metrics"]["artifact_reads"], 1)
        self.assertEqual(controller.state["metrics"]["artifact_cache_hits"], 1)
        self.assertEqual(controller.state["metrics"]["artifact_bytes_read"], self.artifact.stat().st_size)

    def test_artifact_cache_evicts_links_absent_from_current_reply(self):
        linked = self.root / "project/linked.md"
        linked.write_text("linked delivery")
        controller = autopilot.Autopilot(autopilot.Config(self.raw))
        scope = controller.config.agents[0]
        self.append_message("assistant", "Read [delivery](%s)." % linked, "final_answer")
        first = controller.context(scope)
        self.assertEqual(len(first["artifacts"]), 2)
        self.append_message("assistant", "The next result has no delivery link.", "final_answer")
        second = controller.context(scope)
        self.assertEqual([a["path"] for a in second["artifacts"]], [str(self.artifact)])
        self.assertEqual(set(controller.artifact_cache[scope.name]), {str(self.artifact)})
        self.assertEqual(controller.state["metrics"]["artifact_reads"], 2)
        self.assertEqual(controller.state["metrics"]["artifact_cache_hits"], 1)

    def test_existing_run_without_artifact_metrics_resumes_without_resetting_totals(self):
        self.control["action"] = "done"
        self.save_control()
        self.run_tick()
        state_path = self.root / "run/state.json"
        state = json.loads(state_path.read_text())
        for name in ("artifact_reads", "artifact_cache_hits", "artifact_bytes_read"):
            del state["metrics"][name]
        previous_reads = state["metrics"]["context_reads"]
        state_path.write_text(json.dumps(state))
        self.append_message("assistant", "A new result after upgrading the controller.", "final_answer")
        self.run_tick()
        metrics = json.loads(state_path.read_text())["metrics"]
        self.assertGreater(metrics["context_reads"], previous_reads)
        self.assertEqual(metrics["judgment_attempts"], 2)
        self.assertEqual(metrics["artifact_reads"], 1)

    def test_warm_artifact_cache_never_masks_missing_or_invalid_files(self):
        original = self.artifact.read_bytes()
        for invalid in (None, b"\xff", b"\x00", b"x" * 65537):
            with self.subTest(content=repr(invalid)[:20]):
                self.artifact.write_bytes(original)
                controller = autopilot.Autopilot(autopilot.Config(self.raw))
                scope = controller.config.agents[0]
                controller.context(scope)
                if invalid is None:
                    self.artifact.unlink()
                else:
                    self.artifact.write_bytes(invalid)
                with self.assertRaises((autopilot.InputError, FileNotFoundError)):
                    controller.context(scope)

    def test_complete_reply_artifacts_default_config_and_exact_tell(self):
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "sent")
        calls = self.rows("judges.jsonl")
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0]["context"]["messages"][-1]["text"], self.body)
        self.assertEqual(calls[0]["context"]["artifacts"][0]["text"], self.artifact.read_text())
        self.assertNotIn("intermediate commentary", json.dumps(calls[0]["context"]["messages"]))
        self.assertIsNone(calls[0]["shifu_id"])
        self.assertEqual(calls[0]["codex_home"], os.environ.get("CODEX_HOME"))
        argv = calls[0]["argv"]
        self.assertNotIn("--model", argv)
        self.assertNotIn("-m", argv)
        self.assertNotIn("--profile", argv)
        self.assertIn("mcp_servers.fixture-server.enabled=false", argv)
        self.assertIn("read-only", argv)
        self.assertIn("--ephemeral", argv)
        inventory = self.rows("inventories.jsonl")[0]
        self.assertEqual(inventory[:4], ["--disable", "plugins", "--disable", "apps"])
        sends = self.rows("sends.jsonl")
        self.assertEqual([r["name"] for r in sends], ["a"])
        self.assertIn(str(self.root / "STOP"), sends[0]["argv"][-1])
        self.assertIn("Do not install", sends[0]["argv"][-1])
        self.assertEqual((self.root / "run/state.json").stat().st_mode & 0o777, 0o600)
        self.assertEqual((self.root / "run/events.jsonl").stat().st_mode & 0o777, 0o600)
        state = json.loads((self.root / "run/state.json").read_text())
        entry = next(iter(state["agents"]["a"]["replies"].values()))
        self.assertNotIn("rejected_decision", entry)
        self.assertNotIn("rejected_decision", events[-1])

    def test_actual_schema_and_prompt_bind_each_judgments_sources(self):
        self.run_tick()
        self.append_message("assistant", "A new delivery changes the source list.", "final_answer")
        self.run_tick()
        calls = self.rows("judges.jsonl")
        self.assertEqual(len(calls), 2)
        for call in calls:
            expected = [m["source_id"] for m in call["context"]["messages"]] + [a["source_id"] for a in call["context"]["artifacts"]]
            evidence = call["schema"]["properties"]["evidence"]
            self.assertEqual(evidence["items"], {"type":"string", "enum":expected})
            self.assertEqual((evidence["minItems"], evidence["maxItems"]), (1, 16))
            for key in ("reason", "next_step"):
                text = call["schema"]["properties"][key]
                self.assertEqual((text["minLength"], text["maxLength"]), (1, 6000))
            for source in expected:
                self.assertIn(source, call["source_notice"])
        self.assertNotEqual(calls[0]["schema"]["properties"]["evidence"], calls[1]["schema"]["properties"]["evidence"])
        self.assertNotIn("enum", autopilot.DECISION_SCHEMA["properties"]["evidence"]["items"])
        self.assertEqual(len(self.rows("sends.jsonl")), 2)

    def test_rejected_evidence_is_classified_saved_and_never_dispatched_or_retried(self):
        for flag, code in [("empty_evidence", "evidence_empty"), ("many_evidence", "evidence_too_many"), ("fake_evidence", "evidence_unknown")]:
            with self.subTest(code=code):
                self.control = {flag:True}; self.save_control()
                self.raw["state_path"] = str(self.root / ("run/" + code + ".json"))
                _, events = self.run_tick()
                event = events[-1]
                self.assertEqual((event["action"], event["error_code"]), ("needs_user", code))
                state = json.loads(Path(self.raw["state_path"]).read_text())
                entry = next(iter(state["agents"]["a"]["replies"].values()))
                self.assertEqual(entry["phase"], "judge_failed")
                self.assertEqual(entry["error_code"], code)
                self.assertEqual(entry["rejected_decision"], event["rejected_decision"])
                self.assertFalse(entry["rejected_decision_redacted"])
                self.assertEqual(entry["supplied_sources"], event["supplied_sources"])
                rejected = event["rejected_decision"]["evidence"]
                if code == "evidence_empty": self.assertEqual(rejected, [])
                elif code == "evidence_too_many": self.assertGreater(len(rejected), 16)
                else: self.assertEqual(rejected, ["artifact:not-read"])
                self.run_tick()
        self.assertEqual(len(self.rows("judges.jsonl")), 3)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_rejected_diagnostic_redacts_credentials_in_state_and_log(self):
        self.control["credential_answer"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertTrue(events[-1]["rejected_decision_redacted"])
        self.assertEqual(events[-1]["rejected_decision"]["api_key"], "[REDACTED]")
        for name in ["run/state.json", "run/events.jsonl"]:
            text = (self.root / name).read_text()
            for credential in ["sk-fixture-secret-12345678", "fixture-private-password", "fixture-private-token-12345", "fixture-inline-secret"]:
                self.assertNotIn(credential, text)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_source_notice_is_included_in_the_context_budget(self):
        cfg = autopilot.Config(self.raw)
        controller = autopilot.Autopilot(cfg)
        context = controller.context(cfg.agents[0])
        self.raw["max_context_bytes"] = len(context["prompt"].encode())
        _, events = self.run_tick()
        self.assertIn("source manifest", events[-1]["reason"])
        self.assertFalse(self.rows("judges.jsonl"))
        self.assertFalse(self.rows("sends.jsonl"))

    def test_restart_deduplicates_processed_reply(self):
        self.run_tick()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["reason"], "reply already processed")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_dry_run_has_separate_state_and_does_not_consume_real_send(self):
        _, events = self.run_tick("--dry-run")
        self.assertEqual(events[-1]["send_state"], "dry_run")
        self.assertFalse(self.rows("sends.jsonl"))
        self.assertFalse((self.root / "run/state.json").exists())
        self.assertTrue((self.root / "run/state.json.dry-run").exists())
        self.run_tick()
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_running_approval_exited_and_starting_never_judge_or_send(self):
        for status in ("running", "needs_approval", "exited", "starting"):
            with self.subTest(status=status):
                self.record["status"] = status; self.save_record()
                _, events = self.run_tick()
                self.assertEqual(events[-1]["action"], "wait" if status in ("running", "starting") else "needs_user")
        self.assertFalse(self.rows("judges.jsonl"))
        self.assertFalse(self.rows("sends.jsonl"))

    def test_status_is_rechecked_after_judgment_then_same_plan_can_resume(self):
        self.control["judge_status"] = "needs_approval"; self.save_control()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "deferred")
        self.assertFalse(self.rows("sends.jsonl"))
        self.save_record()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "sent")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)

    def test_newer_user_turn_prevents_stale_followup(self):
        self.control["judge_user"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_changed_artifact_defers_dispatch(self):
        self.control["judge_artifact"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "deferred")
        self.assertFalse(self.rows("sends.jsonl"))

    def test_unknown_send_is_not_retried_even_if_a_new_reply_arrives(self):
        self.control["tell_unknown"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertEqual(events[-1].get("send_state"), "unknown", events)
        self.append_message("assistant", "A later reply does not prove whether the prior command was submitted.", "final_answer")
        _, events = self.run_tick()
        self.assertEqual(events[-1]["action"], "needs_user")
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_interrupted_sending_entry_is_blocked_on_restart(self):
        cfg = autopilot.Config(self.raw)
        state = {"version":2, "run_id":cfg.run_id, "config_hash":cfg.identity, "agents":{"a":{"uncertain_send":"old-reply", "replies":{"old-reply":{"phase":"sending"}}}}}
        autopilot.write_json(self.root / "run/state.json", state)
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "unknown")
        self.assertFalse(self.rows("sends.jsonl"))

    def test_send_timeout_is_unknown_and_never_replayed(self):
        self.raw["command_timeout_seconds"] = 2
        self.control["tell_sleep"] = 6; self.save_control()
        _, first = self.run_tick()
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertEqual(next(iter(state["agents"]["a"]["replies"].values()))["phase"], "unknown", first)
        _, events = self.run_tick()
        self.assertEqual(events[-1].get("send_state"), "unknown", events)
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_exit_zero_without_target_receipt_is_still_unknown(self):
        self.control["tell_no_ack"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "unknown")
        self.run_tick()
        self.assertEqual(len(self.rows("sends.jsonl")), 1)

    def test_stop_during_judgment_prevents_tell(self):
        self.control["judge_stop"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertTrue(events[-1]["stopped"])
        self.assertFalse(self.rows("sends.jsonl"))

    def test_deadline_during_judgment_stops_it_before_tell(self):
        self.assert_deadline_cancels_started_judgment()

    def test_deadline_during_judgment_after_slow_status_startup(self):
        self.control["status_sleep"] = 1.1
        self.assert_deadline_cancels_started_judgment()

    def assert_deadline_cancels_started_judgment(self):
        self.control["judge_sleep"] = 4; self.save_control()
        with patch.dict(os.environ, self.env):
            controller = autopilot.Autopilot(autopilot.Config(self.raw), quiet=True)
        started = self.root / "judge.pid"
        before_deadline = time.time()
        # Expire only after the real fixture judge starts, independent of CLI startup speed.
        with patch.object(autopilot.time, "time", side_effect=lambda: controller.config.deadline + 1 if started.exists() else before_deadline):
            controller.run(once=True)
        self.assertTrue(started.exists(), "deadline scenario must start a judgment")
        events = [json.loads(line) for line in controller.log_path.read_text().splitlines()]
        self.assertTrue(events[-1]["stopped"])
        self.assertFalse(self.rows("sends.jsonl"))
        state = json.loads((self.root / "run/state.json").read_text())
        entry = next(iter(state["agents"]["a"]["replies"].values()))
        self.assertTrue(entry["judgment"]["cancelled"])
        self.assertIsNone(entry["judgment"]["usage"])
        self.assertGreater(state["metrics"]["judgment_seconds"], 0)
        self.assertAlmostEqual(state["metrics"]["judgment_seconds"], entry["judgment"]["elapsed_seconds"], delta=0.001)

    def test_local_delivery_link_is_read_but_outside_link_is_not(self):
        linked = self.root / "project/linked.md"; linked.write_text("linked delivery must be read completely")
        outside = self.root / "outside.md"; outside.write_text("outside material must not enter context")
        self.append_message("assistant", "交付 [本线文件](<%s:12>)，另有[范围外文件](%s)。" % (linked, outside), "final_answer")
        self.run_tick("--dry-run")
        artifacts = self.rows("judges.jsonl")[0]["context"]["artifacts"]
        self.assertEqual([a["path"] for a in artifacts], [str(self.artifact), str(linked)])
        self.assertEqual(artifacts[-1]["text"], linked.read_text())

    def test_expired_and_initial_stop_marker_do_not_start_commands(self):
        self.raw["deadline"] = (datetime.now(timezone.utc) - timedelta(seconds=1)).isoformat()
        _, events = self.run_tick()
        self.assertTrue(events[-1]["stopped"])
        self.raw["deadline"] = (datetime.now(timezone.utc) + timedelta(hours=1)).isoformat()
        (self.root / "STOP").touch()
        _, events = self.run_tick()
        self.assertTrue(events[-1]["stopped"])
        self.assertFalse(self.rows("judges.jsonl"))
        self.assertFalse(self.rows("sends.jsonl"))

    def test_judgment_timeout_is_bounded_and_not_automatically_retried(self):
        self.raw["judge_timeout_seconds"] = 1
        self.control["judge_sleep"] = 4; self.save_control()
        self.run_tick()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["action"], "needs_user")
        self.assertIn("no automatic retry", events[-1]["reason"])
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertFalse(self.rows("sends.jsonl"))
        with self.assertRaises(ProcessLookupError):
            os.kill(int((self.root / "judge.pid").read_text()), 0)

    def test_missing_or_oversize_artifact_and_oversize_reply_do_not_get_summarized(self):
        self.raw["max_artifact_bytes"] = 10
        _, events = self.run_tick()
        self.assertIn("full-read budget", events[-1]["reason"])
        self.raw["max_artifact_bytes"] = 65536
        self.raw["max_context_bytes"] = 100
        self.raw["state_path"] = str(self.root / "run/context-budget-state.json")
        _, events = self.run_tick()
        self.assertIn("nothing was truncated", events[-1]["reason"])
        self.assertFalse(self.rows("judges.jsonl"))

    def test_session_identity_mismatch_is_not_retargeted(self):
        self.record["session_id"] = "replacement-session"; self.save_record()
        _, events = self.run_tick()
        self.assertIn("identity changed", events[-1]["reason"])
        self.assertFalse(self.rows("judges.jsonl"))
        self.assertFalse(self.rows("sends.jsonl"))

    def test_out_of_scope_plan_and_invented_evidence_are_rejected(self):
        self.control["outside_path"] = True; self.save_control()
        _, events = self.run_tick()
        self.assertIn("outside allowed_paths", events[-1]["reason"])
        self.assertFalse(self.rows("sends.jsonl"))
        scope = autopilot.Config(self.raw).agents[0]
        with self.assertRaises(autopilot.InputError):
            autopilot.validate_decision({"action":"done", "reason":"done", "next_step":"nothing", "evidence":["invented"], "target_paths":[]}, scope, {"actual"})

    def test_wait_needs_user_and_done_are_recorded_without_tell(self):
        for action in ("wait", "needs_user", "done"):
            with self.subTest(action=action):
                self.raw["state_path"] = str(self.root / ("run/state-" + action + ".json"))
                self.control["action"] = action; self.save_control()
                _, events = self.run_tick()
                self.assertEqual(events[-1]["action"], action, events)
                self.run_tick()
        self.assertEqual(len(self.rows("judges.jsonl")), 3)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_two_whitelisted_agents_are_independent_and_no_others_are_sent(self):
        second = dict(self.raw["agents"][0], name="b", session_id="session-b", goal="Finish the other focused line.")
        self.raw["agents"].append(second)
        (self.root / "home/agents/b.json").write_text(json.dumps(dict(self.record, name="b", session_id="session-b", pane_id="%2")))
        text = self.transcript.read_text().replace("session-a", "session-b")
        (self.root / "transcripts/rollout-session-b.jsonl").write_text(text)
        self.run_tick()
        self.assertCountEqual([r["name"] for r in self.rows("sends.jsonl")], ["a", "b"])

    def test_single_owner_lock_and_changed_config_are_not_silently_bypassed(self):
        with autopilot.run_lock(self.root / "run/state.json.lock"):
            result, _ = self.run_tick(ok=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("another controller", result.stderr)
        self.run_tick()
        self.raw["agents"][0]["goal"] = "a materially different goal"
        result, _ = self.run_tick(ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("different configuration", result.stderr)

    def test_recent_reader_handles_duplicates_partial_tail_and_same_300_char_prefix(self):
        first = autopilot.recent_reply(self.transcript, "session-a", 1024 * 1024, 6)
        self.assertEqual(first["latest"]["text"], self.body)
        self.assertEqual(len([m for m in first["messages"] if m["role"] == "assistant"]), 1)
        self.append_message("assistant", self.body[:500] + "different ending", "final_answer")
        second = autopilot.recent_reply(self.transcript, "session-a", 1024 * 1024, 6)
        self.assertNotEqual(first["reply_id"], second["reply_id"])
        with self.transcript.open("ab") as f: f.write(b'{"type":"unfinished')
        third = autopilot.recent_reply(self.transcript, "session-a", 1024 * 1024, 6)
        self.assertEqual(third["reply_id"], second["reply_id"])

    def test_partial_transcript_waits_and_resumes_without_an_extra_judgment(self):
        self.control["action"] = "done"
        self.save_control()
        with self.transcript.open("ab") as stream:
            stream.write(b'{"type":"event')
        self.run_control("--start")
        self.wait_for(lambda: self.background_event("a").get("reason") == "waiting for the rest of a transcript record")
        self.assertEqual(self.run_control("--status")["status"], "running")
        self.assertFalse(self.rows("judges.jsonl"))
        with self.transcript.open("ab") as stream:
            stream.write(b'_msg","payload":{}}\n')
        self.wait_for(lambda: self.run_control("--status")["stop_reason"] == "completed")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_partial_supervisor_pause_holds_all_work_until_it_becomes_a_takeover(self):
        lead = self.enable_supervisor()
        row = json.dumps({"type": "response_item", "payload": {"type": "message", "role": "user", "content": "Pause the run."}}).encode()
        with lead.open("ab") as stream:
            stream.write(row[:-5])
        self.run_control("--start")
        self.wait_for(lambda: any(e["reason"] == "waiting for the rest of a transcript record" for e in self.rows("run/events.jsonl")))
        self.assertEqual(self.run_control("--status")["status"], "running")
        with lead.open("ab") as stream:
            stream.write(row[-5:] + b"\n")
        self.wait_for(lambda: self.run_control("--status")["status"] == "stopped")
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertTrue(state["supervisor"]["user_guard"]["stopped"])
        self.assertFalse(self.rows("judges.jsonl"))
        self.assertFalse(self.rows("sends.jsonl"))

    def test_malformed_complete_record_still_blocks(self):
        with self.transcript.open("ab") as stream:
            stream.write(b'{"type":broken}\n')
        _, events = self.run_tick()
        self.assertEqual(events[-1]["action"], "needs_user")
        self.assertIn("invalid complete JSONL", events[-1]["reason"])
        self.assertFalse(self.rows("sends.jsonl"))

    def test_identical_completion_text_after_a_new_user_turn_is_a_new_reply(self):
        first = autopilot.recent_reply(self.transcript, "session-a", 1024 * 1024, 6)
        self.append_message("user", "A separate task can produce the same final words.")
        self.append({"type":"event_msg", "payload":{"type":"task_complete", "last_agent_message":self.body}})
        second = autopilot.recent_reply(self.transcript, "session-a", 1024 * 1024, 6)
        self.assertIsNotNone(second)
        self.assertNotEqual(first["reply_id"], second["reply_id"])

    def test_pause_with_final_ack_before_first_tick_cannot_rebind_authorization(self):
        self.append_message("user", "暂停，不要继续开发。")
        self.append_message("assistant", "好的，已暂停。", "final_answer")
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.assertFalse(self.rows("judges.jsonl"))
        self.assertFalse(self.rows("sends.jsonl"))

    def test_human_pause_with_final_ack_stays_stopped_across_restarts(self):
        self.control["action"] = "wait"; self.save_control()
        self.run_tick()
        self.append_message("user", "暂停，不要继续开发。")
        self.append_message("assistant", "好的，已暂停。", "final_answer")
        _, first = self.run_tick()
        self.append_message("assistant", "另一个任务也已完成。", "final_answer")
        _, second = self.run_tick()
        self.assertEqual(first[-1]["send_state"], "human_stopped")
        self.assertEqual(second[-1]["observed_user"], first[-1]["observed_user"])
        state = json.loads((self.root / "run/state.json").read_text())
        self.assertTrue(state["agents"]["a"]["user_guard"]["stopped"])
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_human_takeover_and_final_reply_during_judgment_blocks_dispatch(self):
        self.control.update(judge_user=True, judge_user_final=True); self.save_control()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.run_tick()
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_takeover_while_agent_running_is_latched_before_status_gate(self):
        self.record["status"] = "running"; self.save_record()
        self.run_tick()
        self.append_message("user", "改做另一项任务。")
        self.append_message("assistant", "新任务已经完成。", "final_answer")
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.record["status"] = "needs_reply"; self.save_record()
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.assertFalse(self.rows("judges.jsonl"))

    def test_successful_own_tell_user_record_allows_the_next_reply(self):
        self.control["tell_echo"] = True; self.save_control()
        self.run_tick()
        self.append_message("assistant", "The controller's scoped check is complete.", "final_answer")
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "sent")
        self.assertEqual(len(self.rows("judges.jsonl")), 2)
        self.assertEqual(len(self.rows("sends.jsonl")), 2)
        first = self.rows("sends.jsonl")[0]
        self.append_message("user", "（test-run转来）" + first["argv"][-1])
        self.append_message("assistant", "A human pasted the same old message.", "final_answer")
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.assertEqual(len(self.rows("sends.jsonl")), 2)

    def test_dry_run_message_is_not_a_confirmed_own_send(self):
        _, events = self.run_tick("--dry-run")
        self.append_message("user", "（test-run转来）" + events[-1]["message"])
        self.append_message("assistant", "This was never a real controller dispatch.", "final_answer")
        _, events = self.run_tick("--dry-run")
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)

    def test_stopped_target_does_not_stop_the_other_target(self):
        self.raw["agents"].append(dict(self.raw["agents"][0], name="b", session_id="session-b"))
        (self.root / "home/agents/b.json").write_text(json.dumps(dict(self.record, name="b", session_id="session-b", pane_id="%2")))
        (self.root / "transcripts/rollout-session-b.jsonl").write_text(self.transcript.read_text().replace("session-a", "session-b"))
        self.append_message("user", "我接手 a，先不要再派发。")
        self.append_message("assistant", "好的。", "final_answer")
        _, events = self.run_tick()
        self.assertEqual([e["send_state"] for e in events], ["human_stopped", "sent"])
        self.assertEqual([s["name"] for s in self.rows("sends.jsonl")], ["b"])

    def test_pause_outside_recent_window_is_still_found_from_the_saved_cursor(self):
        self.raw["max_transcript_bytes"] = 8192
        self.control["action"] = "wait"; self.save_control()
        self.run_tick()
        self.append_message("user", "暂停，接下来由我处理。")
        for _ in range(40):
            self.append({"type":"response_item", "payload":{"type":"function_call_output", "output":"x" * 512}})
        self.append_message("assistant", "好的，已暂停。", "final_answer")
        _, events = self.run_tick()
        self.assertEqual(events[-1]["send_state"], "human_stopped")
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_old_state_without_user_guard_is_not_automatically_migrated(self):
        cfg = autopilot.Config(self.raw)
        original = {"version":1, "run_id":cfg.run_id, "config_hash":cfg.identity, "agents":{}}
        autopilot.write_json(self.root / "run/state.json", original)
        result, _ = self.run_tick(ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("different configuration", result.stderr)
        self.assertEqual(json.loads((self.root / "run/state.json").read_text()), original)
        self.assertFalse(self.rows("judges.jsonl"))

    def progress_controller(self, results=None):
        import hashlib
        self.progress_path = self.root / "project/progress.json"
        self.proof = self.root / "project/receipt.json"
        self.proof.write_text('{"req":"one","received":true}')
        self.result = {"id":"one", "evidence":str(self.proof), "sha256":hashlib.sha256(self.proof.read_bytes()).hexdigest()}
        self.progress_path.write_text(json.dumps({"batch":"first", "results":[self.result] if results is None else results}))
        self.raw["agents"][0]["progress"] = {"path":str(self.progress_path), "target":3, "stalled_after_seconds":10}
        controller = autopilot.Autopilot(autopilot.Config(self.raw))
        return controller, controller.config.agents[0]

    def test_receipts_not_counter_or_repaint_advance_progress(self):
        controller, scope = self.progress_controller()
        with patch.object(autopilot.time, "time", return_value=1000): controller.observe_progress(scope)
        state = controller.state["progress"]["a"]
        self.assertEqual((state["completed"],state["last_progress_at"]), (1,1000))
        self.progress_path.write_text(self.progress_path.read_text())
        with patch.object(autopilot.time, "time", return_value=1011): controller.observe_progress(scope)
        self.assertEqual((state["phase"],state["last_progress_at"]), ("stalled",1000))
        self.assertIn("调整",controller.progress_notices()["a"]["next_step"])
        self.progress_path.write_text(json.dumps({"batch":"first","results":[self.result],"completed":99}))
        controller.observe_progress(scope)
        self.assertEqual(state["phase"],"invalid")
        self.assertIsNone(state["completed"])

    def test_live_desktop_review_usage_updates_before_any_new_receipt(self):
        self.progress_controller(results=[])
        self.control.update(action="wait", judge_sleep=1, usage={"input_tokens":25, "output_tokens":0})
        self.save_control()
        self.run_control("--start")
        path = self.root / "home/supervision/a.json"

        def review():
            return autopilot.read_json(path).get("review", {}) if path.is_file() else {}

        self.wait_for(lambda: review().get("recorded_judgments") == 1 and review()["reported_judgments"] == 0)
        self.assertIsNone(review()["input_tokens"])
        self.wait_for(lambda: review().get("reported_judgments") == 1)
        self.assertEqual(review(), {"recorded_judgments":1, "reported_judgments":1,
                                    "input_tokens":25, "cached_input_tokens":None, "output_tokens":0})
        self.assertEqual(self.run_control("--status")["status"], "running")
        self.assertEqual(autopilot.read_json(path)["completed"], 0)

    def test_desktop_review_usage_counts_prior_attempts_in_its_own_conversation(self):
        controller, scope = self.progress_controller()
        controller.state["agents"] = {
            "a": {"replies": {
                "first": {"prior_judgments":[{"usage":{"input_tokens":10, "cached_input_tokens":3}}],
                          "judgment":{"usage":{"input_tokens":20, "output_tokens":4}}},
                "second": {"judgment":{"usage":None, "cancelled":True}},
            }},
            "another": {"replies":{"other":{"judgment":{"usage":{"input_tokens":1000, "output_tokens":700}}}}},
        }
        controller.observe_progress(scope)
        progress = autopilot.read_json(self.root / "home/supervision/a.json")
        self.assertEqual(progress["completed"], 1)
        self.assertEqual(progress["review"], {"recorded_judgments":3, "reported_judgments":2,
                                              "input_tokens":30, "cached_input_tokens":None, "output_tokens":None})

    def test_a_damaged_generated_desktop_display_is_rebuilt_from_verified_work(self):
        self.progress_controller()
        self.control["action"] = "wait"
        self.save_control()
        display = self.root / "home/supervision/a.json"
        display.parent.mkdir()
        display.write_bytes(b'{"phase":\xff')
        self.run_tick()
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        rebuilt = autopilot.read_json(display)
        self.assertEqual((rebuilt["completed"], rebuilt["phase"]), (1, "stopped"))
        self.assertEqual(self.run_control("--status")["progress_display_errors"], {})

    def test_blocked_display_recovers_while_idle_and_keeps_the_original_file(self):
        self.progress_controller()
        self.raw["poll_seconds"] = 1
        self.record["status"] = "running"
        self.save_record()
        parent = self.root / "home/supervision"
        parent.write_text("existing file")
        self.run_control("--start")
        self.wait_for(lambda: bool(self.run_control("--status")["progress_display_errors"].get("a")))
        status = self.run_control("--status")
        self.assertEqual((status["status"], status["progress"]["a"]["completed"]), ("running", 1))
        self.assertEqual(parent.read_text(), "existing file")
        parent.rename(parent.with_name("preserved-file"))
        parent.mkdir()
        self.wait_for(lambda: (parent / "a.json").is_file() and not self.run_control("--status")["progress_display_errors"])
        self.assertEqual(autopilot.read_json(parent / "a.json")["completed"], 1)
        self.assertEqual(self.rows("judges.jsonl"), [])

    def test_fifo_in_the_display_path_preserves_the_pipe_and_verified_work(self):
        self.progress_controller()
        self.control["action"] = "wait"
        self.save_control()
        display = self.root / "home/supervision/a.json"
        display.parent.mkdir()
        os.mkfifo(display)
        self.run_tick()
        status = self.run_control("--status")
        self.assertTrue(display.is_fifo())
        self.assertEqual(status["progress"]["a"]["completed"], 1)
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertIn("regular file", status["progress_display_errors"]["a"])

    def test_stall_clock_does_not_reread_unchanged_proof_and_notices_its_return(self):
        controller, scope = self.progress_controller()
        with patch.object(Path, "read_bytes", autospec=True, side_effect=Path.read_bytes) as reads:
            with patch.object(autopilot.time, "time", return_value=1000):
                controller.observe_progress(scope)
            with patch.object(autopilot.time, "time", return_value=1010):
                controller.observe_progress(scope)
                controller.observe_progress(scope)
            state = controller.state["progress"]["a"]
            self.assertEqual((state["phase"], reads.call_count), ("stalled", 1))

            self.proof.unlink()
            controller.observe_progress(scope)
            self.assertEqual(state["phase"], "invalid")
            self.proof.write_text('{"req":"one","received":true}')
            controller.observe_progress(scope)
            self.assertEqual((state["phase"], state["completed"], reads.call_count), ("stalled", 1, 2))

    def test_growing_reports_hash_new_receipts_and_recheck_atomically_replaced_files(self):
        controller, scope = self.progress_controller()
        second = self.proof.with_name("two.json")
        second.write_bytes(b"second outcome")
        row = {"id": "two", "evidence": str(second), "sha256": autopilot.hashlib.sha256(second.read_bytes()).hexdigest()}
        with patch.object(Path, "read_bytes", autospec=True, side_effect=Path.read_bytes) as reads:
            controller.observe_progress(scope)
            autopilot.write_json(self.progress_path, {"batch": "first", "results": [self.result, row]})
            controller.observe_progress(scope)
            self.assertEqual([call.args[0] for call in reads.call_args_list], [self.proof, second])
            self.assertEqual(controller.state["progress"]["a"]["completed"], 2)
            # A changed declaration still compares against the actual digest of the unchanged file.
            wrong = dict(row, sha256="0"*64)
            autopilot.write_json(self.progress_path, {"batch": "first", "results": [self.result, wrong]})
            controller.observe_progress(scope)
            self.assertEqual(reads.call_count, 2)
            self.assertIsNone(controller.state["progress"]["a"]["completed"])
            # A same-size replacement with its predecessor's mtime is a new file.
            stat_info = second.stat()
            replacement = second.with_suffix(".new")
            replacement.write_bytes(b"x"*stat_info.st_size)
            os.utime(replacement, ns=(stat_info.st_atime_ns, stat_info.st_mtime_ns))
            replacement.replace(second)
            controller.observe_progress(scope)
            self.assertEqual([call.args[0] for call in reads.call_args_list], [self.proof, second, second])
            self.assertIsNone(controller.state["progress"]["a"]["completed"])

    def test_dry_run_progress_never_creates_or_replaces_a_live_projection(self):
        self.progress_controller()
        live = self.root / "home/supervision/a.json"
        self.control["action"] = "wait"
        self.save_control()
        self.run_tick("--dry-run")
        self.assertFalse(live.exists())
        dry_state = json.loads((self.root / "run/state.json.dry-run").read_text())
        self.assertEqual(dry_state["progress"]["a"]["completed"], 1)

        live.parent.mkdir()
        live.write_text('{"run_id":"active-run","phase":"met"}')
        old = live.stat()
        self.run_tick("--dry-run")
        self.assertEqual((live.stat().st_ino, live.stat().st_mtime_ns), (old.st_ino, old.st_mtime_ns))

    def test_same_receipt_cannot_be_recredited_in_next_batch(self):
        controller, scope = self.progress_controller()
        controller.observe_progress(scope)
        self.progress_path.write_text(json.dumps({"batch":"second", "results":[dict(self.result,id="renamed")] }))
        controller.observe_progress(scope)
        self.assertEqual(controller.state["progress"]["a"]["completed"],0)

    def test_desktop_receipt_belongs_to_an_outcome_counted_in_this_batch(self):
        controller, scope = self.progress_controller()
        view = self.root / "home/supervision/a.json"
        controller.observe_progress(scope)
        self.assertEqual(autopilot.read_json(view)["receipt"], self.result)
        second = self.proof.with_name("second receipt.json")
        second.write_bytes(b"new outcome")
        new = {"id":"two", "evidence":str(second), "sha256":autopilot.hashlib.sha256(second.read_bytes()).hexdigest()}
        autopilot.write_json(self.progress_path, {"batch":"next", "results":[self.result, new]})
        controller.observe_progress(scope)
        self.assertEqual(autopilot.read_json(view)["receipt"], new)
        second.unlink()
        controller.observe_progress(scope)
        self.assertNotIn("receipt", autopilot.read_json(view))
        autopilot.write_json(self.progress_path, {"batch":"next", "results":[self.result]})
        controller.observe_progress(scope)
        empty = autopilot.read_json(view)
        self.assertEqual(empty["completed"], 0)
        self.assertNotIn("receipt", empty)

    def test_relabeling_the_same_receipt_cannot_refresh_the_stall_clock(self):
        controller, scope = self.progress_controller()
        with patch.object(autopilot.time, "time", return_value=1000): controller.observe_progress(scope)
        self.progress_path.write_text(json.dumps({"batch":"first", "results":[dict(self.result,id="renamed")]}))
        with patch.object(autopilot.time, "time", return_value=1011): controller.observe_progress(scope)
        state = controller.state["progress"]["a"]
        self.assertEqual((state["last_progress_at"],state["phase"]),(1000,"stalled"))

    def test_changed_missing_or_duplicate_receipt_is_not_a_healthy_count(self):
        controller, scope = self.progress_controller()
        controller.observe_progress(scope)
        self.proof.write_text('{"req":"different","received":true}')
        controller.observe_progress(scope)
        self.assertIsNone(controller.state["progress"]["a"]["completed"])
        self.assertEqual(controller.state["progress"]["a"]["phase"],"invalid")
        self.progress_path.write_text(json.dumps({"batch":"first", "results":[self.result,self.result]}))
        self.proof.write_text('{"req":"one","received":true}')
        controller.observe_progress(scope)
        self.assertIsNone(controller.state["progress"]["a"]["completed"])
        self.proof.unlink()
        controller.observe_progress(scope)
        self.assertIsNone(controller.state["progress"]["a"]["completed"])

    def test_replacing_a_receipt_and_its_declared_digest_cannot_redefine_a_verified_outcome(self):
        self.progress_controller()
        self.record["status"] = "running"
        self.save_record()
        original = self.proof.read_bytes()
        self.run_tick()
        first = self.run_control("--status")["progress"]["a"]
        self.proof.write_bytes(b"another outcome")
        changed = dict(self.result, sha256=autopilot.hashlib.sha256(self.proof.read_bytes()).hexdigest())
        autopilot.write_json(self.progress_path, {"batch":"first", "results":[changed]})
        self.run_tick()
        invalid = self.run_control("--status")["progress"]["a"]
        self.assertEqual((invalid["completed"], invalid["receipt_hashes"]), (None, {"one": self.result["sha256"]}))
        self.assertIn("previously verified receipt changed", invalid["error"])
        self.proof.write_bytes(original)
        autopilot.write_json(self.progress_path, {"batch":"first", "results":[self.result]})
        self.run_tick()
        restored = self.run_control("--status")["progress"]["a"]
        self.assertEqual((restored["completed"], restored["last_progress_at"]), (1, first["last_progress_at"]))
        self.assertEqual(self.rows("judges.jsonl"), [])

    def test_legacy_credit_uses_verified_bytes_to_reconstruct_the_id_to_digest_pairs(self):
        controller, scope = self.progress_controller()
        another = self.proof.with_name("second.json")
        another.write_bytes(b"another result")
        second = {"id":"two", "evidence":str(another), "sha256":autopilot.hashlib.sha256(another.read_bytes()).hexdigest()}
        rows = [self.result, second]
        autopilot.write_json(self.progress_path, {"batch":"first", "results":rows})
        controller.observe_progress(scope)
        state = controller.state["progress"]["a"]
        del state["receipt_hashes"]
        state["seen"] = ["two", "one"]
        state["seen_hashes"] = [self.result["sha256"], second["sha256"]]
        original_time = state["last_progress_at"]
        self.proof.write_bytes(b"replacement")
        changed = dict(self.result, sha256=autopilot.hashlib.sha256(self.proof.read_bytes()).hexdigest())
        autopilot.write_json(self.progress_path, {"batch":"first", "results":[second, changed]})
        controller.observe_progress(scope)
        self.assertIsNone(state["completed"])
        self.assertNotIn("receipt_hashes", state)
        self.proof.write_bytes(b'{"req":"one","received":true}')
        autopilot.write_json(self.progress_path, {"batch":"first", "results":rows})
        controller.observe_progress(scope)
        self.assertEqual(state["receipt_hashes"], {row["id"]:row["sha256"] for row in rows})
        self.assertEqual((state["completed"], state["last_progress_at"]), (2, original_time))
        # Earlier versions with no recorded hashes can verify the live proof and continue.
        del state["receipt_hashes"], state["seen_hashes"]
        controller.progress_stamps.clear()
        controller.observe_progress(scope)
        self.assertEqual((state["completed"], state["last_progress_at"]), (2, original_time))
        autopilot.write_json(self.progress_path, {"batch":"first", "results":[dict(second, id="one"), dict(self.result, id="two")]})
        controller.observe_progress(scope)
        self.assertIsNone(state["completed"])
        self.assertEqual(state["receipt_hashes"], {row["id"]:row["sha256"] for row in rows})

    def test_progress_is_observed_while_agent_runs_and_does_not_finish_the_controller(self):
        self.progress_controller(results=[])
        self.record["status"]="running"; self.save_record()
        self.run_tick()
        state=json.loads((self.root / "run/state.json").read_text())
        self.assertEqual(state["progress"]["a"]["completed"],0)
        self.assertFalse(self.rows("judges.jsonl"))
        self.assertFalse(self.rows("sends.jsonl"))


    def test_running_supervisor_queue_receipt_is_not_human_takeover(self):
        self.enable_supervisor(status="running")
        self.raw["queue_supervisor"] = True
        controller, scope = self.progress_controller(results=[])
        controller.env["AUTOPILOT_FIXTURE"] = str(self.root)
        controller.config_path = self.config_path
        with patch.object(autopilot.time,"time",return_value=1000): controller.observe_progress(scope)
        with patch.object(autopilot.time,"time",return_value=1011): controller.observe_progress(scope)
        notice = controller.progress_notices()
        self.assertFalse(controller.notify_supervisor(notice))
        self.assertEqual(len(self.rows("sends.jsonl")),1)
        self.assertFalse(controller.notify_supervisor(notice))
        self.assertEqual(len(self.rows("sends.jsonl")),1)
        entry = next(iter(controller.state["supervisor"]["replies"].values()))
        path = self.root / "transcripts/rollout-session-lead.jsonl"
        with path.open("a") as f: f.write(json.dumps({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":entry["user_message"]}]}})+"\n")
        self.assertFalse(controller.supervisor_guard())
        receipt_path=self.root / "home/inbox/lead" / (entry["input_id"]+".json")
        receipt=json.loads(receipt_path.read_text());receipt["phase"]="received";receipt_path.write_text(json.dumps(receipt))
        self.assertFalse(controller.supervisor_guard())
        self.assertEqual(entry["phase"],"sent")
        with path.open("a") as f: f.write(json.dumps({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Pause this run"}]}})+"\n")
        self.assertTrue(controller.supervisor_guard())

    def test_stall_notification_does_not_end_an_active_run(self):
        self.progress_controller(results=[])
        self.raw["agents"][0]["progress"]["stalled_after_seconds"]=1
        self.record["status"]="running";self.save_record()
        self.enable_supervisor()
        self.control["tell_echo"]=True;self.save_control()
        self.run_control("--start")
        self.wait_for(lambda:len(self.rows("sends.jsonl"))==1)
        time.sleep(1.1)
        status=self.run_control("--status")
        self.assertEqual(status["status"],"running")
        self.assertEqual(len(self.rows("sends.jsonl")),1)
        self.assertFalse(self.rows("judges.jsonl"))

    def test_stall_notices_survive_restart_and_batch_renames_until_new_proof(self):
        self.enable_supervisor(status="running")
        self.raw["queue_supervisor"] = True
        self.control["queue_phase"] = "received"
        self.save_control()
        controller, scope = self.progress_controller(results=[])

        def observe(now):
            with patch.object(autopilot.time, "time", return_value=now):
                controller.observe_progress(scope)

        def notify():
            controller.env["AUTOPILOT_FIXTURE"] = str(self.root)
            controller.config_path = self.config_path
            controller.supervisor_probe = 0
            return controller.notify_supervisor(controller.progress_notices())

        observe(1000)
        observe(1011)
        self.assertTrue(notify())
        self.assertEqual(len(self.rows("sends.jsonl")), 1)
        restored = autopilot.Autopilot(autopilot.Config(self.raw))
        restored.state = autopilot.read_json(controller.state_path)
        controller, scope = restored, restored.config.agents[0]
        for batch in ("second", "third"):
            autopilot.write_json(self.progress_path, {"batch":batch, "results":[]})
            observe(1012)
            self.assertTrue(notify())
        self.assertEqual(len(self.rows("sends.jsonl")), 1)
        self.assertEqual(controller.state["progress"]["a"]["batch"], "third")
        autopilot.write_json(self.progress_path, {"batch":"third", "results":[self.result]})
        observe(1020)
        self.assertFalse(controller.progress_notices())
        observe(1031)
        self.assertTrue(notify())
        self.assertEqual(len(self.rows("sends.jsonl")), 2)
        self.assertEqual(len(controller.state["supervisor"]["replies"]), 2)

    def test_old_stall_receipt_still_identifies_the_same_stall_after_upgrade(self):
        self.enable_supervisor()
        controller, scope = self.progress_controller(results=[])
        controller.env["AUTOPILOT_FIXTURE"] = str(self.root)
        with patch.object(autopilot.time, "time", return_value=1000): controller.observe_progress(scope)
        with patch.object(autopilot.time, "time", return_value=1011): controller.observe_progress(scope)
        prior = controller.progress_notices()["a"]
        prior["progress"].pop("verified_total", None)
        prior.update(send_state="handled", reply_id=None)
        old_report = {"a":prior}
        old_key = autopilot.digest(old_report)
        controller.state["supervisor"] = {"replies":{old_key:{"phase":"sent", "report":old_report}}}
        autopilot.write_json(self.progress_path, {"batch":"renamed", "results":[]})
        with patch.object(autopilot.time, "time", return_value=1012): controller.observe_progress(scope)
        self.assertTrue(controller.notify_supervisor(controller.progress_notices()))
        self.assertEqual(self.rows("sends.jsonl"), [])
        self.assertEqual(list(controller.state["supervisor"]["replies"]), [old_key])

    def test_immediate_native_receipt_is_not_mistaken_for_human_takeover(self):
        self.enable_supervisor(status="running")
        self.raw["queue_supervisor"] = True
        self.control["queue_phase"] = "received"; self.save_control()
        controller, scope = self.progress_controller(results=[])
        controller.env["AUTOPILOT_FIXTURE"] = str(self.root)
        controller.config_path = self.config_path
        with patch.object(autopilot.time, "time", return_value=1000): controller.observe_progress(scope)
        with patch.object(autopilot.time, "time", return_value=1011): controller.observe_progress(scope)
        self.assertTrue(controller.notify_supervisor(controller.progress_notices()))
        self.assertFalse(controller.supervisor_guard())

    def test_unchanged_idle_agent_still_reaches_the_stall_threshold(self):
        self.progress_controller(results=[])
        self.raw["agents"][0]["progress"]["stalled_after_seconds"] = 1
        self.control["action"] = "wait"; self.save_control()
        self.enable_supervisor()
        self.control["tell_echo"] = True; self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: len(self.rows("sends.jsonl")) == 1)
        self.assertEqual(self.run_control("--status")["status"], "running")


    def test_empty_new_batches_do_not_hide_a_stall(self):
        controller, scope = self.progress_controller(results=[])
        with patch.object(autopilot.time,"time",return_value=1000): controller.observe_progress(scope)
        self.progress_path.write_text(json.dumps({"batch":"second","results":[]}))
        with patch.object(autopilot.time,"time",return_value=1011): controller.observe_progress(scope)
        state=controller.state["progress"]["a"]
        self.assertEqual((state["completed"],state["started_at"],state["last_progress_at"],state["phase"]),(0,1000,0,"stalled"))


    def test_judge_cannot_finish_below_the_receipt_target(self):
        self.progress_controller(results=[])
        self.control["action"]="done";self.save_control()
        self.run_tick()
        state=json.loads((self.root / "run/state.json").read_text())
        self.assertEqual(state["progress"]["a"]["phase"],"stalled")
        self.assertEqual(state["last_events"]["a"]["action"],"wait")

    def test_late_receipt_reopens_only_the_held_completion_and_preserves_usage(self):
        self.progress_controller(results=[])
        self.raw["agents"][0]["progress"]["target"] = 1
        self.control.update(action="done", usage={"input_tokens": 51, "output_tokens": 3})
        self.save_control()
        self.run_tick()
        saved = json.loads((self.root / "run/state.json").read_text())
        entry = next(iter(saved["agents"]["a"]["replies"].values()))
        self.assertEqual((entry["phase"], entry["decision"]["action"]), ("awaiting_progress", "done"))
        self.run_tick()
        self.assertEqual(len(self.rows("judges.jsonl")), 1)

        self.progress_path.write_text(json.dumps({"batch": "first", "results": [self.result]}))
        self.run_control("--start")
        self.wait_for(lambda: self.run_control("--status")["stop_reason"] == "completed")
        final = self.run_control("--status")
        self.assertEqual(final["progress"]["a"]["completed"], 1)
        self.assertEqual(final["usage"], {"reported_judgments": 2, "input_tokens": 102, "cached_input_tokens": None, "output_tokens": 6})
        self.assertFalse(self.rows("sends.jsonl"))

    def test_missing_receipt_recovers_while_the_controller_remains_available(self):
        self.progress_controller()
        self.control["action"] = "wait"
        self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: (self.background_event("a").get("action") == "wait"
                               and len(self.rows("judges.jsonl")) == 1))
        data = self.proof.read_bytes()
        self.proof.unlink()
        self.wait_for(lambda: self.run_control("--status")["progress"]["a"]["phase"] == "invalid")
        self.assertEqual(self.run_control("--status")["status"], "running")
        self.proof.write_bytes(data)
        self.wait_for(lambda: self.run_control("--status")["progress"]["a"]["completed"] == 1)
        self.assertEqual(len(self.rows("judges.jsonl")), 1)
        self.assertFalse(self.rows("sends.jsonl"))

    def test_old_shortfall_state_recovers_after_restart(self):
        self.progress_controller(results=[])
        self.raw["agents"][0]["progress"]["target"] = 1
        self.control["action"] = "done"
        self.save_control()
        self.run_tick()
        path = self.root / "run/state.json"
        state = json.loads(path.read_text())
        entry = next(iter(state["agents"]["a"]["replies"].values()))
        entry.update(phase="handled", decision={**entry["decision"], "action": "wait", "reason": autopilot.PROGRESS_SHORTFALL})
        path.write_text(json.dumps(state))
        self.progress_path.write_text(json.dumps({"batch": "first", "results": [self.result]}))
        _, events = self.run_tick()
        self.assertEqual(events[-1]["action"], "done")
        self.assertEqual(len(self.rows("judges.jsonl")), 2)

    def test_done_is_rechecked_when_evidence_changes_during_judgment(self):
        self.progress_controller()
        self.raw["agents"][0]["progress"]["target"] = 1
        self.control.update(action="done", judge_hold_agents=["a"])
        self.save_control()
        self.run_control("--start")
        self.wait_for(lambda: len(self.rows("judges.jsonl")) == 1)
        self.artifact.write_text("Updated delivery that the first judgment did not see.")
        (self.root / "release-a").touch()
        self.wait_for(lambda: self.run_control("--status")["stop_reason"] == "completed")
        judged = self.rows("judges.jsonl")
        self.assertEqual(len(judged), 2)
        self.assertEqual(judged[-1]["context"]["artifacts"][0]["text"], self.artifact.read_text())



if __name__ == "__main__":
    unittest.main()
