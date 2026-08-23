---
name: engage
description: "Entry skill — auto-detects task complexity and selects the right gear (G1/G2/G3) to balance speed vs discipline"
---

# SHIFU: Engage

You are running SHIFU — an adaptive discipline system. Before every task, determine the right gear.

## The Gear System

Every task runs at one of three gears. **Announce the gear before starting work.**

**G1 (Light):** Direct action. Do the work, run verification, done.
- Single-file edits, typos, config changes, small bug fixes
- Requirements are crystal clear
- Follows existing patterns

**G2 (Medium):** Quick design + disciplined execution.
- Multi-file features (3-5 files), moderate complexity
- Requirements are mostly clear but need a brief design pass
- Invoke: `shifu:think-first` (quick mode) → `shifu:test-first` → `shifu:review` (single-round)

**G3 (Heavy):** Full process.
- Large features (6+ files), architecture changes, unclear requirements
- Invoke: `shifu:think-first` (full mode) → `shifu:finish` (worktree) → `shifu:test-first` → `shifu:review` (two-round) → `shifu:finish`

## Gear Detection

| Signal | G1 | G2 | G3 |
|--------|----|----|-----|
| Estimated files changed | 1-2 | 3-5 | 6+ |
| Requirements clarity | Clear | Mostly clear | Vague/exploratory |
| Architecture impact | None | Local | Structural |
| Existing patterns to follow | Yes | Partial | No/new domain |

**Conflict resolution:** When signals disagree, pick the gear indicated by the *highest-risk* signal — usually "Architecture impact" or "Requirements clarity" beats "file count." Example: 2 files (G1) but vague requirements (G3) → use G2, not G3. Jumping to G3 for 2 files is ceremony waste; G2 gives you a quick design pass without the full G3 process. Only go to G3 when two+ signals agree.

**Exception — new domain, small scope:** When the task touches a new domain/pattern but only 3-5 files, prefer G2 with a thorough `think-first` pass rather than escalating to full G3. The overhead of worktree + two-round review is not justified unless requirements are also vague or architecture is structural.

**User language hints:**
- "fix this", "change X to Y", "update the config" → G1
- "add a feature", "implement X" → G2
- "build a system", "I want to create...", "redesign the..." → G3

## Gear Context Rules

Each gear has a context discipline — don't load what you don't need.

| Resource | G1 | G2 | G3 |
|----------|----|----|-----|
| Memory files | Compact index only | + relevant topic files | Full tiered memory |
| System prompt | Minimal (~60 lines active) | Moderate | Full spec context |
| Tools | Default set | Expand on demand | Full + MCP + subagents |
| Back-pressure | Quick typecheck/lint | Build + lint | Full test suite + coverage |

**Rule:** When in doubt about loading more context, don't. You can always upgrade mid-task. See `shifu:context` for detailed guidance.

## Manual Override

Users can override at any time:
- **"G1" / "G2" / "G3"** — switch to that gear
- **"upgrade"** — shift up one gear
- **"downgrade"** — shift down one gear
- **"skip the process"** — drop to G1 for this task

User instructions always take precedence over gear recommendations.

## Skill Routing

| Gear | research | think-first | test-first | review | orchestrate | finish | migrate |
|------|----------|-------------|------------|--------|-------------|--------|---------|
| G1 | quick lookup | skip | run existing tests | self-check + verify | skip | optional | full |
| G2 | comparative scan | quick mode | full TDD | single-round review | skip | optional | full |
| G3 | deep analysis | full mode | full TDD | two-round review | optional (5+ tasks) | required | full |

**Research-first tasks:** When the task is investigation, comparison, or technology evaluation (not implementation), route to `shifu:research` BEFORE `shifu:think-first`. Signals: "should we use X or Y?", "what's the best approach?", "research X", "compare these options", "evaluate".

## Learnings

If the session-start injected an "Accumulated Learnings" index, check it before starting work. If any learning matches the current task's domain, load it with `cat <path>` and apply the pattern. Don't load all learnings — only relevant ones.

## All Gears Must

**YOU MUST** do these regardless of gear — no exceptions:
1. **Announce gear + plan** — state the gear and what you will do before starting
2. **Verify before claiming done** — run the command, read the output, THEN claim success
3. **Not introduce security vulnerabilities** — validate inputs at boundaries
4. **Not break existing tests** — run the test suite before finishing
5. **Patch-first on SKILL.md** — use `Edit` (targeted replacement) not `Write` (full rewrite) when modifying skill files. Small diffs are reviewable; full rewrites lose change context.

## Context Management Signals

Invoke `shifu:context` explicitly when you notice:
- Conversation has exceeded ~30 turns
- You are re-reading the same files multiple times
- Tool results are truncating or context feels heavy
- You catch yourself forgetting an earlier constraint

You do NOT need to invoke `shifu:context` on every task — it is a rescue tool, not a startup ritual.

## Red Flags — Wrong Gear

- Spending 10+ minutes on a "G1 task" → upgrade to G2
- Writing 100+ lines without a plan on "G2" → upgrade to G3
- Running full brainstorming for a one-line fix → downgrade to G1
- User seems frustrated by process overhead → downgrade

**When signals are a tie, prefer the lower gear.** It's cheaper to add process than to undo wasted ceremony. This is consistent with the conflict-resolution rule above: only upgrade when multiple signals agree, not when one signal "feels" serious.

## Model Tier Delegation

Gear picks **process depth**. Model tier picks **which model** for each step.

| Gear | Default tier for main step | Notes |
|------|--------------------------|-------|
| G1 | T1 (Haiku) | Simple narrow edits. Upgrade only if wrong. |
| G2 | T2 (Sonnet) | Most coding, planning, reviewing. |
| G3 | T3 (Opus) for design/architecture, T2 for implementation, T1 for scaffolding | Rare: all-T3 G3. Most G3 steps are still T2. |

See `shifu:delegate` for the full tier rules, upgrade/downgrade triggers, and per-task priors.

**Announce both gear AND tier** when starting: `Gear: G2, Tier: T2 for code, T1 for tests.`
