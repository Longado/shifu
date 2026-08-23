---
name: review
description: "Use when work is done — adaptive code review + verification. Scales from self-check (G1) to two-round review (G3)"
---

# SHIFU: Review

> **Gears:** G1 (self-check), G2 (single-round review), G3 (two-round review). See `shifu:engage` for gear definitions.

## The Iron Law

```
PROVE IT, THEN SAY IT
```

YOU MUST attach evidence from this response to every success claim. No command output = no claim. "Should work" is not evidence.

## Gear Adaptation

### G1: Self-Check
1. Run typecheck/lint — errors-only output (suppress warnings)
2. Run existing tests for changed files only
3. Review your own diff — `git diff` or `git diff --staged`
4. Report actual results with evidence

No subagent needed.

### G2: Single-Round Review
1. Run build + lint + affected tests (not full suite)
2. Dispatch ONE code-reviewer subagent with the template at `review/reviewer-prompt.md`
3. The reviewer checks BOTH spec compliance AND code quality in one pass
4. Fix Critical/Important issues, re-verify
5. Report with evidence

### G3: Two-Round Review
1. Run full test suite + coverage report + security scan (if available)
2. **Round 1 — Spec Compliance:** Dispatch a reviewer subagent (using `review/reviewer-prompt.md`) focused on: does code match requirements? Nothing extra, nothing missing.
3. Fix spec issues. Re-verify.
4. **Round 2 — Code Quality:** Dispatch a second reviewer subagent focused on: architecture, patterns, security, performance.
5. Fix quality issues. Re-verify.
6. Report with evidence

If subagent dispatch is unavailable, perform both rounds inline using the reviewer template as a mental checklist.

## Verification Gate

Before ANY success claim, run this gate:

```
1. What command would prove this? (identify)
2. Run it now — fresh, complete, no shortcuts
3. Read the FULL output — exit code, failure count, warnings
4. Does the output support the claim?
   - NO → report what actually happened
   - YES → state claim with the evidence attached
5. Only after step 4: make the claim
```

## Outcome Logging

At the END of a review (after all fixes are applied), log the outcome so SHIFU can track which skills find real issues vs noise:

```bash
SCRIPT="${CLAUDE_PLUGIN_ROOT}/hooks/scripts/metrics.sh"
# N = number of real issues found and fixed (0 if review found nothing)
bash "$SCRIPT" log "review_outcome" "<gear>" "<N>"
```

- `N=0` → review found nothing (pure overhead this session)
- `N>=1` → review caught at least one real issue (signal)

This feeds the health CLI's skill signal ratio: `review: 3/5 sessions found issues (60% signal)`.

## Receiving Review Feedback

When a reviewer (human or subagent) gives feedback:

**When responding to reviewer feedback (human or subagent):**

**DO:**
- Read complete feedback before reacting
- Verify suggestions against codebase reality
- Push back with technical reasoning when reviewer is wrong
- Fix one item at a time, test each fix
- Say "Fixed. [what changed]" — actions over words

**DO NOT:**
- "You're absolutely right!" / "Great point!" / "Thanks for catching that!" (performative)
- Implement without verifying suggestion is correct
- Implement partial feedback — clarify ALL unclear items first
- Accept "implement properly" without checking if the feature is actually used (YAGNI)

## Red Flags — STOP

- Using "should", "probably", "seems to" about test results
- Saying "Done!" or "Perfect!" before running verification
- About to commit/push without running tests
- Trusting a subagent's success report without checking the diff
- Thinking "just this once I'll skip verification"

## Rationalization Prevention

| Excuse | Reality |
|--------|---------|
| "Should work now" | RUN the verification command. |
| "I'm confident" | Confidence is not evidence. |
| "Linter passed so it builds" | Linter does not check compilation. Run the build. |
| "Agent said success" | Verify the diff independently. |
| "Partial check is enough" | Partial proves nothing about the whole. |
