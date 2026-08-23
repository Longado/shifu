---
name: debug
description: "Use when encountering any bug, test failure, or unexpected behavior — requires root cause investigation before proposing fixes"
---

# SHIFU: Debug

> **Gears:** All gears. See `shifu:engage` for gear definitions.

## The Iron Law

```
UNDERSTAND BEFORE YOU FIX
```

YOU MUST explain WHY the bug happens before proposing ANY solution. Symptoms are not causes.

## The Four Phases

### Phase 1: Evidence

**BEFORE attempting ANY fix:**

1. **Read error messages carefully** — full stack trace, line numbers, error codes. Don't skim.
2. **Reproduce consistently** — exact steps, does it happen every time?
3. **Check recent changes** — git diff, recent commits, new dependencies, config changes.
4. **Trace data flow** — where does the bad value originate? Trace backward through the call stack until you find the source. Fix at source, not at symptom.
5. **In multi-component systems** — add diagnostic logging at each component boundary. Run once to gather evidence. THEN analyze.

### Tracing Toolkit

When gathering evidence, use these concrete techniques:

**Stack trace instrumentation:** Add temporary `console.error()` / `print()` / logging BEFORE the suspected operation (not after). Include stack traces (`new Error().stack` in JS, `traceback.print_stack()` in Python). Run once, read output, remove after diagnosis.

**Four-layer validation:** When the bug crosses component boundaries, add checks at each layer:
1. **Input boundary** — what enters the system?
2. **Transform boundary** — what does processing produce?
3. **Storage boundary** — what gets persisted?
4. **Output boundary** — what leaves the system?

Run with all 4 active. The first layer that sees bad data reveals the source.

**Condition-based waiting:** NEVER use `sleep(N)` to "fix" race conditions or flaky tests. Instead, poll for the expected condition with a loud-failure timeout:
```
# Instead of sleep(2), use:
wait_until(condition=lambda: result is not None, timeout=5s, message="result ready")
```

### Phase 2: Pattern

1. **Find working examples** — similar working code in the same codebase.
2. **Compare** — what's different between working and broken? List every difference.
3. **Understand dependencies** — what settings, config, environment does this need?

### Phase 3: Hypothesis

1. **Form one hypothesis** — "I think X is the root cause because Y." Write it down.
2. **Test minimally** — smallest possible change, one variable at a time.
3. **Verify** — did it work? Yes → Phase 4. No → new hypothesis. DON'T stack fixes.

### Phase 4: Fix

1. **Create failing test** — reproduce the bug as a test case (use `shifu:test-first`).
2. **Implement single fix** — address root cause, ONE change. No unrelated "while I'm here" improvements. If the fix requires restructuring adjacent code, explain why before proceeding.
3. **Verify** — test passes? No other tests broken? Issue actually resolved?
4. **If fix doesn't work** — return to Phase 1 with new information.

## The 3-Strike Rule

**If 3+ fixes have failed:**

STOP. Do not attempt fix #4. This pattern means the architecture is wrong:
- Each fix reveals a new problem in a different place
- Fixes require "massive refactoring"
- Each fix creates new symptoms

**Question fundamentals:** Is this pattern sound? Should we refactor the architecture instead of patching symptoms? Discuss with the user before proceeding.

## Red Flags — STOP and Return to Phase 1

- "Quick fix for now, investigate later"
- "Just try changing X and see"
- "I don't fully understand but this might work"
- Proposing solutions before tracing data flow
- "One more fix attempt" (when already tried 2+)

## Rationalization Prevention

| Excuse | Reality |
|--------|---------|
| "Issue is simple, I can just fix it" | Simple bugs still have a cause. Finding it takes 30 seconds; guessing costs 30 minutes. |
| "Emergency, no time" | Systematic debugging is FASTER than guess-and-check. |
| "I see the problem, let me fix it" | Seeing symptoms is not understanding root cause. |
| "Multiple fixes at once saves time" | Can't isolate what worked. Creates new bugs. |
| "One more try" (after 2+ failures) | 3+ failures = architectural problem. Stop fixing, start questioning. |
