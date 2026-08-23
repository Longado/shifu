---
name: llm-contract
description: "Pre-flight checklist for LLM calls — seam design, probability collapse defense, drift engineering. Run before writing any new LLM call, API wrapper, prompt template, or agent loop."
---

# SHIFU: LLM Contract

> **Gears:** All gears. Applies whenever code calls an LLM.

## The Iron Law

```
LLM DOES ONE JUDGMENT. CODE DOES ALL LOOPS.
Judgments must be explicit (structured in, structured out, with reasoning).
Loops must be deterministic (idempotent, retryable, observable).
```

## When This Skill Fires

**Trigger (any):**
- Writing new LLM call (API, SDK, agent loop, prompt template)
- Reviewing existing AI product architecture
- User reports "AI system is slow / expensive / flaky / magic"

**Skip:**
- One-off script / prototype / demo
- Pure LLM usage (no code wrapping)

## The 9-Point Checklist

Score the call. ≥7/9 ships. <7/9 = gambling.

### Seam Design (3 points)

**1. Minimal input**
- ❌ Dump full context / documents / conversation history
- ✅ Single atomic unit + necessary background only
- Challenge: Strip each field — does judgment quality still hold ≥95%? Strip it.

**2. Structured output + reasoning**
- ❌ Free-form text, caller parses
- ✅ Schema-enforced: `{reasoning, verdict, confidence}` — note field order: reasoning BEFORE verdict. LLMs generate left-to-right, forcing reasoning first improves verdict quality.
- Binary outputs (PASS/FAIL, yes/no) beat fine-grained scores (1-5) in production — models can't calibrate fine-grained well.
- Challenge: Can downstream code `switch(result.verdict)`? If not, rewrite.

**3. "Unknown" exit**
- ❌ Forced binary classification → model hallucinates
- ✅ Two idioms for graceful uncertainty:
  - Literal enum with unknown: `verdict: Literal["yes", "no", "unknown"]` + `reason_missing_info` when unknown
  - Optional fields: `extracted_amount: Optional[float] = None` — model returns None when data isn't in input, instead of fabricating
- Pydantic example:
  ```python
  class ExtractResult(BaseModel):
      reasoning: str  # comes first — forces model to think before committing
      extracted_amount: Optional[float] = None
      verdict: Literal["extracted", "unknown"]
      reason_missing_info: Optional[str] = None
  ```
- Challenge: When input is insufficient, can the model say "I don't know"? If no, you built a hallucination generator.

### Composition Defense (3 points)

**4. Call chain ≤ 3 steps**
- Math: 3x 95% chained = 86%. 10x = 60% (coin flip).
- ❌ Autonomous 20-step agent
- ✅ Compress to 2-3 steps with richer context + code adjudication at critical nodes

**5. Redundancy at critical judgments**
- ❌ Single LLM verdict directly triggers side effects
- ✅ Parallel vote (best-of-3 lifts 80% → 94%) or independent LLM cross-check
- Applies to: deletions, payments, permission changes, outbound messages

**6. No loops / aggregation / arithmetic inside LLM**
- ❌ "Let Claude filter these 500 expired items"
- ✅ Code WHERE filter → LLM per-item judgment → code aggregate

### Drift Engineering (3 points)

**7. Prompt in git + code review**
- ❌ Prompts scattered as runtime strings
- ✅ Dedicated file, versioned, diffed, with changelog

**8. Pin model version**
- ❌ `model: "claude-sonnet-latest"` → Anthropic ships update, your behavior drifts silently
- ✅ `model: "claude-sonnet-4-6"` → upgrades are explicit, gated by eval

**9. Eval harness + production drift monitoring (three layers)**

**Layer A — Deterministic code assertions** (fast CI fail, <10s)
- Schema validation, required fields, format checks, deterministic invariants
- Runs on every PR. Fail-fast gate before anything more expensive.

**Layer B — LLM-as-judge** (regression guard, async)
- Golden set: 100-200 labeled examples (good/bad output pairs)
- Judge prompt with reasoning-first, binary PASS/FAIL (not 1-5)
- Target: ~85% agreement with human labels (1-α=0.85)
- Known pitfall: judge agreeableness bias — research shows TPR >96% but TNR <25%. Mitigate with 3-5 labeled few-shot examples inside the judge prompt (halves residual error).
- Runs after Layer A passes, target <5 min.

**Layer C — Production drift monitoring** (always-on sampling)
- Sample N% of live traffic
- Track distributions: `confidence`, output length, `unknown` rate, refusal rate, token cost
- Alert on >1σ shift vs rolling 7-day baseline
- Research signal: unmonitored LLM systems see ~35% error rate increase over 6 months
- Write model version + prompt version into every record's metadata for drift forensics

## Scoring (per-point PASS/FAIL)

Binary per point — don't sum into a weighted score, the model (and you) can't calibrate that anyway.

| Rule | PASS | FAIL |
|------|------|------|
| 1. Minimal input | Every field justified, nothing speculative | Dumping context/docs/history |
| 2. Structured output + reasoning | Schema with reasoning field FIRST, binary verdict | Free-form text / fine-grained score |
| 3. Unknown exit | `unknown` literal or `Optional[T]=None` possible | Forced binary, no escape hatch |
| 4. Chain ≤3 steps | ≤3 LLM calls in sequence per user turn | 4+ steps, or "let the agent figure it out" |
| 5. Redundancy at critical judgments | Vote/cross-check on any side-effect call | Single judgment triggers action |
| 6. No loops in LLM | Code does all iteration | `for item in list: llm(item)` lives inside one LLM call |
| 7. Prompt in git | Dedicated prompt file, versioned, reviewed | Inline string / framework-buried / undocumented |
| 8. Pin model version | Explicit `claude-sonnet-4-6` | `-latest` / alias / unpinned |
| 9. Eval harness 3-layer | A + B + C all present | Missing any layer |

**Verdict:**
- 9 PASS → ship-ready
- Any FAIL → either fix before shipping, or **log the accepted risk explicitly** and move on (don't pretend it's fine)

Don't compute a total score. "7/9" sounds acceptable and hides which violations actually hurt.

## Anti-Patterns Quick Reference

| Symptom | Violation | Fix |
|---------|-----------|-----|
| Agent breaks halfway | #4 (chain too long) | Compress steps, add code checkpoints |
| Same prompt works today, fails tomorrow | #8 (no pinning) | Lock model ID |
| Occasional hallucinated outputs | #3 (no unknown exit) | Add unknown + reason_missing_info |
| Prompt edit breaks downstream | #7 + #9 | Prompt in git + eval CI |
| Cost explosion | #6 (LLM doing loops) | Push loops back to code layer |

## The Three New Roles

A mature LLM-native team needs three specialists most companies skip:

1. **Seam architect** — decides the code/LLM boundary and designs contracts
2. **Eval engineer** — golden sets, LLM-as-judge, drift monitoring
3. **Prompt maintainer** — treats prompts as code with changelog + review

Missing any of these = "demos great, doesn't deliver."

## Integration with Other Skills

- **shifu:think-first** — run this skill during design phase when LLM is involved. Ratify contract before coding.
- **shifu:review** — reviewer uses this checklist as a gate when reviewing LLM-touching code.
- **shifu:research** — if chosen LLM approach scores ≤6/9, research alternatives before proceeding.
- **shifu:debug** — when debugging flaky LLM behavior, map symptom to violation via Anti-Patterns table.

## Anti-Anti-Patterns

| Temptation | Don't | Do |
|-----------|-------|-----|
| "Model is smart enough to figure it out" | Pass raw context and hope | Constrain input, force reasoning output |
| "Just add more retries" | Hide flakiness | Fix the contract (violation of #2, #3, or #4) |
| "We'll add eval later" | Ship without eval | No eval = you don't know when it regresses; block ship |
| "Let the agent decide" | Let the LLM orchestrate | Orchestration is code's job. LLM is a function. |

## Future Upgrade Path

This skill is a conceptual gate. Natural evolutions:
- CLI static checker (`llm-check path/to/code.py`) that flags violations on real code
- GitHub Action scoring PRs touching LLM calls
- Per-project config file defining which points are mandatory vs. advisory

Ship the conceptual version first; upgrade when a concrete project needs it.
