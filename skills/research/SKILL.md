---
name: research
description: "Gear-aware research methodology — scales from quick lookup (G1) to deep comparative analysis (G3) with mandatory contrarian review"
---

# SHIFU: Research

> **Gears:** G1 (quick lookup), G2 (comparative scan), G3 (deep analysis). See `shifu:engage` for gear definitions.

Research before you conclude. Scale the research effort to match the decision stakes.

## The Iron Law

```
NO OPINION WITHOUT EVIDENCE. NO ADOPTION WITHOUT CONTRARIAN REVIEW.
```

## Gear Adaptation

### G1: Quick Lookup (5 min)

Single-source answer. "What is X?" or "Does Y support Z?"

1. **Search** — one web search or doc read
2. **Answer** — direct answer with source link
3. **Confidence tag** — HIGH / MEDIUM / LOW based on source quality
4. **Done**

No comparison matrix. No opinion. Just facts.

### G2: Comparative Scan (15-30 min)

Evaluating 2-4 options. "Should we use X or Y?"

1. **Frame** — state the decision and constraints in one sentence
2. **Scan options** — research each option sequentially (or parallel agents if orchestrate is enabled)
3. **Comparison matrix** — build a table with dimensions that matter (not vanity metrics)
4. **Subjective call** — state your recommendation with reasoning
5. **Devil's advocate** — one paragraph: why your recommendation might be wrong
6. **Output** — matrix + recommendation + risk

### G3: Deep Analysis (1-2 hours)

Architecture-level research. "What memory system should we build?"

**Phase A: Landscape Map**
1. Identify all relevant projects/approaches (aim for 5-10)
2. Classify into tiers by originality and relevance
3. Parallel deep-dive on top 3 (subagents)

**Phase B: Architecture Extraction**
For each top candidate:
1. Core model — what's the central metaphor/abstraction?
2. Key design decisions — what trade-offs did they make and why?
3. Structural diagram — ASCII or markdown table of components
4. Limitations — what breaks at scale? What's the failure mode?

**Phase C: Synthesis**
1. **Cross-cutting comparison** — matrix of all candidates on dimensions that matter
2. **Insight extraction** — what non-obvious patterns emerge across projects?
3. **Contrarian review** (MANDATORY) — challenge every conclusion:
   - "What if the opposite is true?"
   - "What evidence would change this recommendation?"
   - "What am I anchored on that might be wrong?"
4. **Applicability filter** — which insights transfer to OUR context vs. their context?

**Phase D: Decision Framework**
1. **Recommendation** — concrete, actionable, phased
2. **Anti-recommendation** — what NOT to do and why
3. **Cherry-pick list** — specific ideas to borrow without adopting the whole system
4. **Open questions** — what we still don't know and how to find out

## Research Quality Rules

### Source Hierarchy
1. Primary source (code, official docs, author's writing) > Secondary (blog posts, tutorials)
2. Benchmark with methodology > Claims without evidence
3. Production experience > Theoretical analysis
4. Recent (< 6 months) > Older (unless foundational)

### Mandatory Outputs by Gear

| Output | G1 | G2 | G3 |
|--------|----|----|-----|
| Source links | 1+ | 3+ | 5+ per candidate |
| Comparison matrix | — | Required | Required |
| Subjective opinion | — | Required | Required |
| Contrarian review | — | 1 paragraph | Full section |
| Cherry-pick list | — | — | Required |
| Open questions | — | — | Required |

### Context Integration

- **Before research:** check MEMORY.md for prior research on this topic
- **Before research (project context):** invoke shifu:research-navigation to check for existing NAVIGATION.md and prior research assets. Do not duplicate work.
- **After research:** if findings are non-obvious and durable, save to memory
- **After research (project context):** if you created new durable findings, update NAVIGATION.md to help future sessions navigate your work
- **Don't save to memory files:** ephemeral facts, benchmark numbers (they change), implementation details (they're in code). Raw benchmarks belong in source files, not compiled memory.
- **Do save:** architectural insights, validated design decisions, surprising findings

## Anti-Patterns

| Anti-Pattern | Fix |
|-------------|-----|
| Researching for 2 hours on a G1 question | Time-box strictly. If you need more, upgrade the gear. |
| Listing 10 projects without ranking | Always tier. The user doesn't have time for a flat list. |
| Recommending without contrarian review | The most dangerous research is the one that confirms your bias. |
| Copying marketing claims as findings | Read the code or the benchmark methodology, not the landing page. |
| "More research needed" as a conclusion | State what you know NOW, flag what's uncertain, give a provisional recommendation. |
| Researching in main context when agents could parallelize | G2+ should use parallel subagents. Main context is for synthesis. |

## Integration with Other Skills

- **shifu:think-first** — research feeds design. Do research BEFORE think-first when domain is unfamiliar.
- **shifu:reflect** — log research friction: "spent 30 min but couldn't find X" is valuable.
- **shifu:context** — research generates lots of context. Use subagents as context firewalls.
- **shifu:campaign** — for multi-session research, persist progress in campaign state.
