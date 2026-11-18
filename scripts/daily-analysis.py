#!/usr/bin/env python3
"""Run one daily-analysis scheduling check; launchd calls this again every five minutes.

Reports are the completion record. A successful `tell` only records delivery, so an
idle agent that produced no report can be retried, at most three times per date.
"""
import argparse
from datetime import date, datetime, time, timedelta
import fcntl
import json
from pathlib import Path
import subprocess
from zoneinfo import ZoneInfo

TZ = ZoneInfo("Asia/Taipei")
READY = {"idle", "needs_reply"}
RETRY_SECONDS = 30 * 60
MAX_ATTEMPTS = 3


def save_state(path, state):
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, ensure_ascii=False, indent=2) + "\n")
    tmp.replace(path)


def notify(message):
    result = subprocess.run(
        ["/usr/bin/osascript", "-e", "on run argv",
         "-e", 'display notification (item 1 of argv) with title "每日分析"',
         "-e", "end run", message], capture_output=True, text=True, timeout=15)
    if result.returncode:
        print(json.dumps({"action": "notification_failed", "error": result.stderr.strip()}, ensure_ascii=False), flush=True)


def run_once(args, now):
    state = json.loads(args.state.read_text()) if args.state.exists() else {"days": {}}
    days = state["days"]
    # Before 09:10 only catch up older reports; yesterday becomes due at 09:10.
    through = args.through or (now.date() - timedelta(days=1 if now.time() >= time(9, 10) else 2))
    due = []
    day = args.first_day
    while day <= through:
        key = day.isoformat()
        report = args.reports / f"{key}.md"
        if report.is_file() and report.stat().st_size > 0:
            days.setdefault(key, {})["outcome"] = "reported"
        else:
            due.append(key)
        day += timedelta(days=1)

    def finish(action, **details):
        event = {"at": now.isoformat(), "action": action, "missing": due, **details}
        if not args.dry_run:
            state["last_check"] = event
            save_state(args.state, state)
            # One notification per changed failure, not on every five-minute check.
            notice = f"{now.date()}:{action}:{details.get('reason', '')}"
            if action in {"waiting", "blocked"} and state.get("notice") != notice:
                state["notice"] = notice
                save_state(args.state, state)
                notify(f"每日分析尚未完成：{details.get('reason', action)}。待处理日期：{', '.join(due)}")
        print(json.dumps(event, ensure_ascii=False), flush=True)
        return event

    if not due:
        return finish("up_to_date")
    result = subprocess.run([str(args.shifu), "status", "--json"], capture_output=True, text=True, check=True, timeout=30)
    conversations = json.loads(result.stdout)
    mine = [c for c in conversations if c.get("task") == args.task and c.get("cwd") == str(args.role_dir)]
    if any(c["status"] == "running" for c in mine):
        return finish("busy", reason="分析会话仍在执行，下次检查")
    ready = [c for c in mine if c["status"] in READY]
    if not ready:
        reason = "没有可接收指令的每日分析会话" if not mine else "会话尚未就绪或在等确认：" + ", ".join(f"{c['name']}={c['status']}" for c in mine)
        return finish("waiting", reason=reason)

    eligible = []
    for key in due:
        record = days.get(key, {})
        if record.get("outcome") in {"sending", "send_unknown"}:
            return finish("blocked", reason=f"{key} 的上次发送结果不明，先核对会话，避免重复发送")
        if record.get("attempts", 0) < MAX_ATTEMPTS:
            eligible.append(key)
    if not eligible:
        return finish("blocked", reason="缺失日期均已尝试三次，仍未生成报告，需要查看分析会话")
    key = eligible[0]
    record = days.get(key, {})
    if now.timestamp() - record.get("last_attempt", 0) < RETRY_SECONDS:
        return finish("cooldown", day=key)
    selected = sorted(ready, key=lambda c: c["name"])[0]
    if args.dry_run:
        return finish("would_send", day=key, agent=selected["name"])

    record = {"attempts": record.get("attempts", 0) + 1, "last_attempt": now.timestamp(),
              "agent": selected["name"], "outcome": "sending"}
    days[key] = record
    save_state(args.state, state)
    prompt = f"开始分析 {key}。按当前目录里的每日分析职责完成这一天，报告保存到 {args.reports / (key + '.md')}。这是自动补跑；只分析指定日期，已有完整报告则不要重复生成。"
    try:
        sent = subprocess.run([str(args.shifu), "tell", selected["name"], "--from", "定时", prompt],
                              capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        record["outcome"] = "send_unknown"
        record["error"] = str(error)
        return finish("blocked", reason=f"{key} 发送结果不明，先核对会话，避免重复发送")
    if sent.returncode:
        record["outcome"] = "send_unknown"
        record["error"] = (sent.stderr or sent.stdout).strip()
        return finish("blocked", reason=f"{key} 发送失败，先核对会话；原因已写入状态文件")
    record["outcome"] = "sent"
    return finish("sent", day=key, agent=selected["name"], attempt=record["attempts"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reports", type=Path, required=True)
    parser.add_argument("--first-day", type=date.fromisoformat, required=True)
    parser.add_argument("--task", default="每日分析")
    parser.add_argument("--role-dir", type=Path, default=Path.home() / ".shifu/app/daily")
    parser.add_argument("--shifu", type=Path, default=Path.home() / ".cargo/bin/shifu")
    parser.add_argument("--state", type=Path, default=Path.home() / ".shifu/daily-analysis-state.json")
    parser.add_argument("--through", type=date.fromisoformat, help="One-off catch-up through this date, including before 09:10")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    now = datetime.now(TZ)
    if args.through and args.through >= now.date():
        parser.error("--through must be before today")
    if args.dry_run:
        run_once(args, now)
        return
    args.state.parent.mkdir(parents=True, exist_ok=True)
    args.reports.mkdir(parents=True, exist_ok=True)
    with args.state.with_suffix(".lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print(json.dumps({"at": now.isoformat(), "action": "already_checking"}), flush=True)
            return
        run_once(args, now)


if __name__ == "__main__":
    main()
