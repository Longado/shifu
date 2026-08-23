---
name: campaign
description: "Persist task state across sessions — gear, phase, task description, session summary, and task queue survive session boundaries"
---

# SHIFU: Campaign

> **Gears:** All gears. Campaign persistence is lightweight and always useful.

## The Iron Law

```
SAVE STATE BEFORE YOU LEAVE, RESTORE STATE WHEN YOU ARRIVE
```

Before ending a session with unfinished work, update the campaign. When starting a session with existing campaign state, announce what was carried over.

## What Campaign Tracks

| Field | Purpose | Example |
|-------|---------|---------|
| gear | Current gear for the task | "G2" |
| phase | Which skill phase you're in | "test-first" |
| task | One-line description of active task | "Add digest system to claude-agents" |
| failureCount | Consecutive failures (circuit breaker) | 2 |
| lastVerification | Last test/build result | "51/51 tests pass" |
| lastSessionSummary | What happened last session | "Wrote tests, implementation pending" |
| queue | Ordered list of upcoming tasks | `[{id, title, gear, status}]` |

## Task Queue (v2)

The queue lets you line up upcoming tasks so SHIFU knows what's next without interrupting the current task.

```bash
SCRIPT="${CLAUDE_PLUGIN_ROOT}/hooks/scripts/campaign.sh"

# Add tasks to the queue
bash "$SCRIPT" queue-add "Fix npm link" "G1"
bash "$SCRIPT" queue-add "Add auth middleware" "G2"

# See what's queued
bash "$SCRIPT" queue-list

# Claim the next task as active (sets .task and .gear)
bash "$SCRIPT" queue-claim 1

# Mark active task done, auto-show next
bash "$SCRIPT" queue-complete

# Peek at next without claiming
bash "$SCRIPT" queue-next

# Clear everything
bash "$SCRIPT" queue-clear
```

**Session start with queue:** If the session-start hook detects queued tasks and no active task, it injects:
```
## Task Queue (2 queued)
Next: #1 [G1] Fix npm link
No active task — say 'start' to claim it.
```
Respond to this by calling `queue-claim <id>` to begin.

## When to Save Campaign

Save campaign state when:
- You detect the session is about to end (user says goodbye, context is heavy)
- You complete a major phase transition (think-first → test-first)
- A G3 task spans multiple sessions
- The user explicitly asks to save progress

**Do NOT save for:**
- G1 tasks (too quick to need persistence)
- Completed tasks (use `queue-complete` or `reset` instead)

## How to Save

Run the campaign script via Bash:

```bash
SCRIPT="${CLAUDE_PLUGIN_ROOT}/hooks/scripts/campaign.sh"

# Update individual fields
bash "$SCRIPT" update gear "G2"
bash "$SCRIPT" update phase "test-first"
bash "$SCRIPT" update task "Add digest system"
bash "$SCRIPT" update lastSessionSummary "Wrote 8 tests, all passing"

# Reset when task is complete (no queue)
bash "$SCRIPT" reset

# Complete and advance queue (preferred when queue exists)
bash "$SCRIPT" queue-complete
```

## When a Session Starts with Campaign State

If the session-start hook injects "Previous Campaign State", you should:

1. **Announce it** — "Resuming from last session: G2, test-first phase, task: ..."
2. **Confirm with user** — "Should I continue from where we left off?"
3. **Resume or reset** — based on user response
4. **Update campaign** — if the task has changed, update the fields

## Integration with Other Skills

- `shifu:engage` — if campaign has a gear, suggest reusing it instead of re-detecting
- `shifu:review` — save lastVerification after running tests
- `shifu:finish` — reset campaign when task is done

## Anti-Patterns

| Anti-Pattern | Fix |
|-------------|-----|
| Saving campaign for every tiny edit | Only save for multi-session tasks (G2/G3) |
| Ignoring campaign state at session start | Always acknowledge and offer to resume |
| Letting campaign go stale for days | Reset if campaign is >48h old |
| Saving sensitive data in campaign | Campaign is plaintext — no secrets, tokens, or passwords |
