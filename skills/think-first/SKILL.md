---
name: think-first
description: "Use before implementation — scales from quick design chat (G2) to full spec + plan (G3)"
---

# SHIFU: Think First

> **Gears:** G2 (quick design), G3 (full spec + plan). G1 skips this skill. See `shifu:engage` for gear definitions.

Design before you build. Scale the design effort to match the task.

## The Iron Law

```
NO IMPLEMENTATION WITHOUT USER-APPROVED DESIGN FIRST
```

This applies to G2 and G3. G1 tasks skip this skill entirely.

## Gear Adaptation

### G2: Quick Design

A focused conversation, NOT a document. Typically a few minutes.

1. **Scan context** — check relevant files, understand current state
2. **Propose approach** — one message: "Here's what I'll do: [approach]. Sound good?"
3. **If complex enough** — offer 2-3 approaches with trade-offs, recommend one
4. **Get user confirmation** — a simple "yes" or "go ahead" is enough
5. **List tasks** — create a TodoWrite checklist of what you'll do
6. **Start building** — invoke `shifu:test-first`

No spec document. No plan file. Just alignment and a checklist.

### G3: Full Design

A thorough design process.

**Phase A: Understand**
1. Explore project context — files, docs, recent commits
2. Ask clarifying questions — one at a time, prefer multiple choice
3. Focus on: purpose, constraints, success criteria
4. If scope is too large — decompose into sub-projects first

**Phase B: Design**
1. Propose 2-3 approaches with trade-offs and your recommendation
2. Present design incrementally — get user approval per section
3. Cover: architecture, components, data flow, error handling, testing
4. Design for isolation — each unit has one purpose, clear interface, testable independently

**Phase C: Document**
1. Write spec to `docs/specs/YYYY-MM-DD-<topic>-design.md` (if directory doesn't exist, create it or ask user for preferred location)
2. **Spec self-review:**
   - Placeholder scan — any "TBD", "TODO", "add validation", "add appropriate handling", vague requirements? Fix them.
   - Internal consistency — do sections contradict each other?
   - Scope check — focused enough for one implementation cycle?
   - Ambiguity check — could any requirement be read two ways? Pick one.
3. Ask user to review the spec before proceeding

**Phase D: Plan**
1. Write implementation plan to `docs/plans/YYYY-MM-DD-<topic>-plan.md`
2. Break into bite-sized tasks (2-5 minutes each)
3. **Every step must include:**
   - Exact file paths
   - Complete code (no placeholders — NO "TBD", "TODO", "add validation", "similar to Task N")
   - Exact commands with expected output
   - TDD steps: write test → verify fails → implement → verify passes → commit
4. **Plan self-review:**
   - Spec coverage — can you point to a task for each requirement?
   - Placeholder scan — any of the banned patterns above?
   - Type consistency — do names match across tasks?
5. **Orchestrate check:** Count the independent subtasks in the plan. If there are 5 or more tasks that can run in parallel — explicitly say: "This plan has N independent tasks. Recommend invoking `shifu:orchestrate` for parallel execution." Do not skip this check.
6. Offer execution choice: subagent-driven via `shifu:orchestrate` (recommended if 5+ tasks) or inline

## Working in Existing Codebases

- Explore current structure before proposing changes
- Follow existing patterns
- If existing code has problems affecting the work, include targeted improvements in the design
- Don't propose unrelated refactoring

## Red Flags

- Starting to write code before design is approved → STOP
- Writing a plan with "TBD" or "add appropriate handling" → fix the placeholder
- Designing for 30 minutes on a G2 task → downgrade the design, upgrade the doing
- User says "just do it" → respect that, drop to G1/G2
