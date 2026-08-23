---
name: orchestrate
description: "G3 only — coordinate parallel subagent execution for large implementation plans with 5+ independent tasks"
---

# SHIFU: Orchestrate

> **Gears:** G3 only. See `shifu:engage` for gear definitions.

## The Iron Law

```
AN APPROVED PLAN MUST EXIST BEFORE ANY SUBAGENT IS DISPATCHED
```

YOU MUST have a written, user-approved plan (from `shifu:think-first`) before dispatching any implementation subagent. No plan = no dispatch.

## When to Use

- Plan has **5+ independent tasks** with clear file boundaries
- Tasks can be worked on without shared state
- You want to parallelize execution within the current session

**Do NOT use when:**
- Tasks are tightly coupled (fix one might affect another)
- Plan has fewer than 5 tasks (use inline sequential execution instead — G3 without orchestrate is valid and often better)
- You don't have an approved plan yet

> **Note:** Most real G3 tasks will have 3-4 independent tasks, which is below the orchestrate threshold. This is expected and correct — G3 without orchestrate is the common path. Orchestrate is for genuinely large plans (system rewrites, multi-module features) where parallelism provides a measurable speedup.

## The Process

### Step 1: Extract Tasks

Read the plan once. Extract all tasks with their full text, file ownership, and test commands. Create a TodoWrite checklist.

### Step 2: Dispatch Per Task

For each task:
1. **Dispatch implementation subagent** with:
   - Full task text (never make subagent read the plan file)
   - File ownership list (which files this task creates/modifies)
   - Test commands and expected output
   - Instruction to follow `shifu:test-first`
2. **Answer subagent questions** before they begin implementation
3. **Subagent completes:** implements, tests, self-reviews, commits

### Step 3: Review Per Task

After each subagent completes:
1. **Dispatch reviewer subagent** using `review/reviewer-prompt.md`
2. If issues found → dispatch fix subagent (same model or higher)
3. Re-review until approved
4. Mark task complete

### Step 4: Final Integration

After all tasks complete:
1. Run full test suite
2. Dispatch final code reviewer for entire implementation
3. Use `shifu:finish` to handle branch completion

## Model Selection

Use the least powerful model that can handle each role:

| Task complexity | Recommended model |
|-----------------|-------------------|
| Isolated, clear spec, 1-2 files | Fast/cheap model |
| Multi-file, integration concerns | Standard model |
| Design judgment, broad codebase | Most capable model |

## Subagent Status Handling

| Status | Action |
|--------|--------|
| **DONE** | Proceed to review |
| **DONE_WITH_CONCERNS** | Read concerns, address if about correctness, then review |
| **NEEDS_CONTEXT** | Provide missing info, re-dispatch |
| **BLOCKED** | Assess: context problem → provide more; too hard → upgrade model; plan wrong → escalate to user |

## Red Flags — STOP

- Dispatching subagents without an approved plan
- Dispatching multiple subagents on the same files (conflicts)
- Skipping review after subagent completion
- Ignoring subagent questions or BLOCKED status
- Starting code quality review before spec compliance passes
- Working on main/master branch without explicit user consent

## Constraints

- **Maximum 3 parallel subagents** — more creates coordination overhead
- **Sequential within dependencies** — if Task B depends on Task A, finish A first
- **Never skip reviews** — every subagent output gets reviewed
