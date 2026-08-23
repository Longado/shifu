---
name: delegate
description: "Model routing — picks the cheapest model that can still do the task well. Complements engage (gear depth) by choosing per-step model tier."
---

# SHIFU: Delegate

> **Gear is about process depth. Delegate is about model tier.**
> Engage picks G1/G2/G3 → Delegate picks Haiku/Sonnet/Opus for each step inside.

---

## The Iron Law

```
USE THE CHEAPEST MODEL THAT STILL HITS THE QUALITY BAR.
```

Default is NOT "always Opus, be safe." Default is "pick one tier lower first, upgrade only when you hit a quality wall."

Why: A session that uses Opus for everything costs **3–15×** more than a well-delegated session. In a 4-hour dev session that's the difference between $2 and $30.

---

## The Tier Table

| Tier | Model | Relative cost | What it's best at |
|------|-------|---------------|-------------------|
| **T1 Light** | Haiku 4.5 | 1× | Classification, routing, extraction, format conversion, simple edits, structured output, re-reads |
| **T2 Balanced** | Sonnet 4.6 | 3× | Coding, multi-step reasoning, code review, writing, refactoring, most planning |
| **T3 Deep** | Opus 4.7 | 12× | Architecture decisions, ambiguous requirements, multi-file refactors, tricky debugging, research synthesis |

Rules of thumb:
- **Start at T2** for unknown tasks. Downgrade to T1 after you see the task is narrow. Upgrade to T3 only when T2 visibly struggles (wrong answer, thin reasoning, missed context).
- **70% of real dev work is T1 or T2.** Only ~30% legitimately needs T3.
- **Never upgrade "to be safe."** Safety through capability is a rationalization for cost.

---

## Task → Tier Map (starting defaults)

| Task type | Tier | Why |
|-----------|------|-----|
| Classify intent / route to skill | T1 | Bounded output, pattern matching |
| Extract structured data from text | T1 | Deterministic transformation |
| Format/convert (md → html, json → table) | T1 | Mechanical |
| Reading and summarizing a file | T1 | Compression, not reasoning |
| Writing a single function + its test | T2 | Coding quality matters |
| Reviewing a diff for bugs | T2 | Needs reasoning but scoped |
| Answering user questions about the codebase | T2 | Varies, T2 is safe default |
| Planning a multi-file feature | T2 | G2-level depth |
| Architecture decision with trade-offs | T3 | Deep reasoning, hard to recover from bad call |
| Debugging with unclear root cause | T3 | High cost of wrong answer |
| Refactor spanning 6+ files | T3 | Systemic risk |
| Writing docs from scratch | T2 | Normally. T1 if template exists |
| Translating existing content | T1 | Not creative |

This table is a **starting prior**, not iron law. The gear-aware rules below override.

---

## Gear × Delegate Cross Table

| Gear | Default tier for the main step | When to upgrade |
|------|--------------------------------|------------------|
| **G1** Light | T1 | Upgrade to T2 if T1 gets the edit wrong once |
| **G2** Medium | T2 for main code, T1 for helpers | Upgrade to T3 if review finds multiple real issues |
| **G3** Heavy | T3 for design/architecture, T2 for implementation, T1 for scaffolding | Downgrade to T2 where the design is settled and only execution remains |

**The trick**: a G3 task does NOT mean every step is Opus. Most steps in G3 are still T2 (write code, write tests) or T1 (scaffold, format). Only the **design + review** steps need T3.

---

## Subagent Delegation (G3 orchestrate)

When `shifu:orchestrate` spawns parallel subagents, pick tier per subagent:

| Subagent role | Default tier |
|---------------|--------------|
| Researcher / info extractor | T1 |
| Coder / implementer | T2 |
| Critic / reviewer | T2 |
| Architect / planner | T3 |
| Coordinator (the one spawning others) | T2 — itself is routing logic, doesn't need T3 |

**Iron sub-law**: The coordinator should almost never be T3. If you find yourself "needing Opus to decide which subagent does what," your plan isn't concrete enough — go back to `shifu:think-first`.

---

## Upgrade Triggers

Upgrade to a higher tier only when one of these is observed:

- Output is factually wrong and a re-run at same tier doesn't fix it
- Output keeps missing context that's clearly in the input
- Reasoning is surface-level on a task that needs depth
- The task has irreversible cost of being wrong (production deploy, migration)
- User explicitly asks for highest quality

**Do NOT upgrade just because**:
- "Task sounds important" (importance ≠ difficulty)
- "I want to be thorough" (thoroughness comes from process, not model)
- "Just in case" (the literal definition of waste)
- "Output feels thin" (often means the prompt is thin, not the model)

---

## Downgrade Triggers

Downgrade to a lower tier when:

- The task is a repeated pattern you've seen before
- The output is a deterministic transformation
- Scope is narrow (one function, one file)
- You're about to call the model 10+ times in a loop
- The test suite itself will catch errors (safety net exists)

---

## Rationalization Prevention

| Excuse | Reality |
|--------|---------|
| "Opus is smarter so it's safer" | Opus costs 12× more. A T2 error + a retry is still 6× cheaper than T3 one-shot. |
| "I don't know which tier, let me just use T3" | "Don't know" → start at T2, not T3. You can upgrade after one miss. |
| "T1 made a mistake last time on similar task" | Similar ≠ same. Try T1 with a tighter prompt before blaming the model. |
| "It's my job to deliver quality, not save money" | Wasted inference IS a quality problem — it slows the session, burns context, teaches users bad habits. |
| "T3 writes better prose" | For reports, the bottleneck is usually sourcing data, not prose. T2 + good data > T3 + weak data. |

---

## Anti-patterns

**Never default to T3.**
If your first instinct on every new task is "Opus for safety," you've stopped thinking. Make yourself justify T3 in words.

**Never use T1 for irreversible decisions.**
T1 is for classification, extraction, transformation. Not for "should we refactor the auth module?"

**Never switch tier mid-task without logging why.**
If you started T2 and moved to T3 because "first attempt was wrong," write one line explaining what T2 missed. This is how you learn tier boundaries.

**Never apply the tier table mechanically.**
"Writing a function → T2" is a prior, not a command. If the function is 3 lines and you've written 50 like it, T1 is fine. If the function is a security-critical crypto primitive, T3 even though "it's just code."

---

## Output Format

When starting work, announce the gear AND the tier, e.g.:

```
Gear: G2
Tier: T2 (Sonnet) for implementation, T1 (Haiku) for scaffolding and test data.
If review finds multiple real issues, upgrade reviewer step to T3.
```

This makes the model choice auditable and correctable by the user.

---

## Users can override

- **"use opus" / "use sonnet" / "use haiku"** → that tier for the next task
- **"don't use opus"** → cap at T2
- **"save cost"** → start one tier lower than default
- **"quality first"** → start one tier higher for critical steps only (not everything)

User instruction always wins.

---

## Relationship to Other Skills

- `shifu:engage` — picks the gear (process depth). Delegate is what you do WITHIN that gear.
- `shifu:orchestrate` — picks how many subagents. Delegate picks what tier each subagent is.
- `shifu:think-first` — the think-first step in G3 is almost always T3. That's the exception, not the rule.

Delegate is always active. It's the quiet cost-aware layer underneath every gear.

---

## Self-audit prompt

End of session, ask:
> "If I replayed this session with optimal tier for each step, would the total cost drop?"

If the honest answer is "yes, by >30%," you defaulted too high. Note which steps were over-tiered and update your priors.
