#!/usr/bin/env python3
"""SHIFU UserPromptSubmit hook — context-tracker

Injects current context token usage as a system reminder, so Claude can
self-observe instead of guessing context fullness based on vibes.

Reads transcript JSONL, finds the latest assistant turn's usage stats, and
emits a brief `[ctx: 213K / 1M (21.4%)]` marker via additionalContext.

Window size detection (in priority order):
  1. SHIFU_CONTEXT_WINDOW env var (e.g. "1000000")
  2. Auto-upgrade: if observed total exceeds 180K, assume 1M window
  3. Default: 200K (Sonnet/standard Opus)

Designed around pull-first context principles:
  - silent on missing data (no noise)
  - one-glance readable (<20 chars)
  - never blocks the prompt
"""
import json
import os
import sys


def get_latest_usage(transcript_path):
    """Return the most recent assistant turn's usage dict, or None."""
    if not transcript_path or not os.path.exists(transcript_path):
        return None
    latest = None
    try:
        with open(transcript_path) as f:
            for line in f:
                try:
                    d = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if d.get("type") == "assistant":
                    u = d.get("message", {}).get("usage")
                    if u:
                        latest = u
    except OSError:
        return None
    return latest


def detect_window(total_tokens):
    """Pick window size: env override, auto-upgrade, or default 200K."""
    env_window = os.environ.get("SHIFU_CONTEXT_WINDOW")
    if env_window and env_window.isdigit():
        return int(env_window)
    if total_tokens > 180_000:
        return 1_000_000
    return 200_000


def format_marker(usage):
    inp = usage.get("input_tokens", 0)
    cc = usage.get("cache_creation_input_tokens", 0)
    cr = usage.get("cache_read_input_tokens", 0)
    total = inp + cc + cr
    if total == 0:
        return None
    window = detect_window(total)
    pct = total / window * 100
    total_k = round(total / 1000)
    window_k = window // 1000
    if window >= 1_000_000:
        window_label = f"{window_k // 1000}M"
    else:
        window_label = f"{window_k}K"
    return f"[ctx: {total_k}K / {window_label} ({pct:.1f}%)]"


def main():
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        sys.exit(0)

    usage = get_latest_usage(data.get("transcript_path", ""))
    if not usage:
        sys.exit(0)

    marker = format_marker(usage)
    if not marker:
        sys.exit(0)

    output = {
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": marker,
        }
    }
    print(json.dumps(output))


if __name__ == "__main__":
    main()
