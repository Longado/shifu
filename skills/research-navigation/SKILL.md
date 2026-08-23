---
name: research-navigation
description: "Navigate existing research assets before starting new research. Prevents duplicate work and surfaces relevant prior context."
---

# SHIFU: Research Navigation

> **Use this skill BEFORE conducting research on a topic.**

## The Rule

```
IF topic has been researched before:
  LOAD existing assets first
  BUILD on top, don't restart
ELSE:
  PROCEED with fresh research
  LOG findings for future navigation
```

## How to Navigate

### Step 1: Check for Navigation Index

Look for `NAVIGATION.md` in these locations:
- `~/study/<topic>/NAVIGATION.md`
- `<project>/docs/research/NAVIGATION.md`
- `<project>/NAVIGATION.md`

If found, read it first. It contains:
- Quick paths by goal ("I need to prepare a pitch", "I need to compare competitors")
- File index with descriptions
- Cross-cutting themes and dependency graphs

### Step 2: Query by Intent

Match your goal to the navigation paths:

| Your Goal | Look For |
|-----------|----------|
| Prepare a presentation | "pitch", "outline", "insights" |
| Understand a concept | "definition", "philosophy", "deep-dive" |
| Compare alternatives | "landscape", "competitors", "frameworks" |
| Build something | "implementation", "architecture", "tools" |
| Apply to a domain | "mapping", "application", "case study" |

### Step 3: Load Context Strategically

**G1 (Quick lookup)**: Read 1-2 summary files only (insights.md, framework.md)
**G2 (Comparative)**: Read standard brief files + relevant deep dives
**G3 (Deep analysis)**: Start with navigation index, then load all relevant files systematically

### Step 4: Update Navigation (if needed)

If your research adds new findings:
- Add the new file to NAVIGATION.md's FILE INDEX
- Tag it in the BY DIMENSION matrix
- If it creates a new cross-cutting theme, add it

## Anti-Patterns

| Anti-Pattern | Fix |
|-------------|-----|
| Starting research without checking NAVIGATION.md | Always check first. 30 seconds saves 30 minutes. |
| Loading all research files into context | Use the navigation system to pick only what you need. |
| Creating a new file without adding it to NAVIGATION.md | Navigation decays if not maintained. Update the index. |
| Duplicating existing research because "I can do it better" | Build on existing work. Cite and extend, don't replace. |

## Integration with shifu:research

- **Before research**: Invoke shifu:research-navigation to check existing assets
- **During research**: If you find gaps in existing research, note them explicitly ("Prior research covered X but not Y")
- **After research**: Update NAVIGATION.md if you created new durable findings

## Session-Start Hint

When SHIFU detects you are in a directory with research assets, it will print:
```
Research navigation available: <path>/NAVIGATION.md (N files)
```
This is a passive hint. You choose whether to load it.
