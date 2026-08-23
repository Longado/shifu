---
name: reflect
description: "Session-end self-reflection — review what happened, log friction, update campaign, consolidate learnings"
---

# SHIFU: Reflect

> **Gears:** All gears. Reflection depth scales with gear.

## The Iron Law

```
EVERY SESSION LEAVES THE SYSTEM SMARTER THAN IT FOUND IT
```

Before a session ends, reflect on what happened. The goal is not ceremony — it's ensuring that friction gets recorded, campaign stays current, and patterns get noticed.

## When to Reflect

- **Always:** When the Stop hook fires (automatic nudge)
- **Manually:** When the user says "wrap up", "that's all", "save progress"
- **Skip if:** Session was trivial (single question, no tool calls, <5 minutes)

## Reflection by Gear

### G1: Quick Check (30 seconds)
- Was the fix correct? Did tests pass?
- Reset campaign if task is done
- No friction log needed unless something unexpected happened

### G2: Brief Review (1 minute)
- What was built? Does it work?
- Was gear detection accurate? (Would G1 or G3 have been better?)
- Log friction if: TDD felt forced, review found nothing, context was insufficient
- Update campaign: lastSessionSummary + lastVerification

### G3: Full Reflection (2 minutes)
- What was the architectural decision and why?
- Did orchestrate/context/think-first add value or just overhead?
- Which skills were loaded but unused? (token waste signal)
- Log friction with specifics: what went wrong, what worked well
- Update campaign: all fields including gear accuracy assessment

## The Reflection Checklist

Run through mentally, act on what applies:

```
[ ] Task status — done, in-progress, or blocked?
[ ] Campaign — update or reset?
[ ] Friction — anything surprising or frustrating?
[ ] Gear accuracy — was the detected gear right?
[ ] Skill value — did loaded skills actually help?
[ ] Verification — did tests/build pass at the end?
```

## How to Record

**Campaign update** (if task continues):
```bash
"${CLAUDE_PLUGIN_ROOT}/hooks/scripts/campaign.sh" update lastSessionSummary "Built digest system, 8 tests passing, CLI registered"
"${CLAUDE_PLUGIN_ROOT}/hooks/scripts/campaign.sh" update phase "review"
```

**Campaign reset** (if task is done):
```bash
"${CLAUDE_PLUGIN_ROOT}/hooks/scripts/campaign.sh" reset
```

**Friction log** (if something was surprising):
```bash
# tasks.db path varies per workspace — check CLAUDE.md or .data/tasks.db
sqlite3 /path/to/.data/tasks.db \
  "INSERT INTO friction_log (description, frequency) VALUES ('[shifu] G2 on X: gear was right but review found nothing useful', 'once');"
```

## Memory Proposals

At session end, evaluate whether the session produced knowledge worth persisting:

**Trigger conditions** (propose if ANY apply):
- Learned something non-obvious about the user's preferences or workflow
- A research finding that changes a prior assumption
- A decision was made that will affect future work
- New evidence for an open question in `research_open_questions.md`

**Proposal format** (show to user, don't auto-write):
```
Memory proposal:
- Type: [user/feedback/project/reference/tracker]
- Summary: [one line]
- Confidence: [high/medium/low]
- Source: [what in this session supports this]
- Action: [NEW file / UPDATE existing file / no action needed]
```

**Rules:**
- NEVER auto-write memory. Always propose and wait for confirmation.
- Batch proposals — show all at once, not one by one.
- If the session was trivial, skip. Don't propose for the sake of proposing.
- Check existing memories first — propose UPDATE over NEW to avoid duplicates.
- Also check `research_open_questions.md` — if the session touched a related topic, propose evidence update.

## Pattern Detection

After 5+ sessions, look for recurring friction:
- Same type of error keeps appearing → candidate for auto-skill
- Same gear override keeps happening → gear detection needs tuning
- Same skill keeps being loaded but unused → conditional activation needed
- Same test pattern keeps being written → template candidate

**Flag patterns to the user:** "I've noticed X happening 3+ times. Should we create a skill for it?"

## Anti-Patterns

| Anti-Pattern | Fix |
|-------------|-----|
| Writing a 500-word reflection essay | 1-3 sentences max. Action over words. |
| Reflecting on trivial sessions | Skip if <5 tool calls and no errors |
| Updating campaign but not friction_log | Friction is more valuable — it drives improvement |
| Performing reflection as performative | Only reflect if there's something actionable |
