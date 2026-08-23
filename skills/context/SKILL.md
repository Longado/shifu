---
name: context
description: "Gear-aware context window management — tiered memory, progressive disclosure, and context rot detection"
---

# SHIFU: Context

> **Gears:** All gears. Context discipline scales with gear. See `shifu:engage` for gear definitions.

## The Iron Law

```
LOAD LESS, VERIFY MORE
```

Every file you read, every memory you load, every tool description active in context competes for attention. Load only what the current gear demands. When unsure, skip it — you can always load later.

## Tiered Memory Loading

Memory systems (MEMORY.md, topic files, project context) should be loaded according to gear:

### G1: Index Only
- Read MEMORY.md index (compact pointers)
- Do NOT open individual memory files unless directly relevant to the one-file fix
- Do NOT load full project architecture docs
- Goal: minimal context, maximum focus

### G2: Index + Relevant Topics
- Read MEMORY.md index
- Open 1-3 topic files that are directly relevant to the feature
- Load project CLAUDE.md for conventions
- Skip architecture docs unless the feature touches structural boundaries
- Goal: enough context to design, not so much that you drift

### G3: Full Tiered Memory
- Read MEMORY.md index
- Load all relevant topic files
- Load architecture docs, design specs, ADRs
- Consider triggering memory consolidation if memories are stale or conflicting
- Goal: comprehensive understanding before committing to architecture

## Progressive Disclosure

Don't front-load context. Load in stages:

```
Stage 1: Task understanding (engage + gear detection)
  → Load: MEMORY.md index, CLAUDE.md only

Stage 2: Design (think-first)
  → Load: relevant topic memories, existing code patterns

Stage 3: Implementation (test-first)
  → Load: specific file context, API docs, test patterns

Stage 4: Review
  → Load: diff only, test output, coverage report
```

Each stage should start with the minimum context needed. If you find yourself scrolling back to re-read something from Stage 1 during Stage 3, that's a sign to compact, not to load more upfront.

## Context Rot Detection

Watch for these signals that context is degrading your performance:

| Signal | Action |
|--------|--------|
| Conversation exceeds ~50 turns | Suggest compact or new session |
| You repeat a mistake you already fixed | Context is too noisy — spawn subagent for isolated work |
| You forget a constraint stated earlier | Re-read the specific constraint, don't re-load everything |
| You're unsure what gear you're in | Re-announce gear and current stage |
| Tool results are getting truncated | You're near context limits — compact immediately |

**When context is getting heavy:**
1. Prefer spawning a subagent (context firewall) over loading more into main context
2. Subagents get a focused brief, do isolated work, return compressed results
3. Main context stays clean for decision-making

## Tool Scope by Gear

| Gear | Tool Loading | Rationale |
|------|-------------|-----------|
| G1 | Use only tools already available | Adding tools costs context; G1 tasks don't need them |
| G2 | Load skills on-demand via `shifu:<name>` | Progressive expansion as complexity reveals itself |
| G3 | Pre-load relevant skills, activate MCP tools, use subagents | Full toolkit justified by task scale |

## Anti-Patterns

| Anti-Pattern | Why It Hurts | Fix |
|-------------|-------------|-----|
| Loading all memory files at session start | Wastes 30%+ of context budget on potentially irrelevant info | Load index only, pull topics as needed |
| Reading entire files when you need one function | Dilutes attention on the target code | Use line ranges or grep first |
| Keeping failed attempt details in context | Noise from dead-end approaches confuses subsequent reasoning | Compact or note the conclusion only |
| Installing "just in case" MCP tools | Each tool description is ~50-100 tokens of permanent context | Add tools only when a task actually needs them |
