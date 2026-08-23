---
name: shifu
description: Use when the user explicitly requests SHIFU, G1, G2, or G3, or when choosing process depth is itself the task.
---

# SHIFU For Codex

SHIFU is an adaptive discipline layer. In Codex, it acts as a router and judgment aid, not a Claude Code plugin.

## Codex Boundary

Claude hooks do not run in Codex. Do not rely on SHIFU `SessionStart`, `PreToolUse`, `PostToolUse`, or `Stop` hooks here.

Use Codex-native skills and tools for enforcement when they are available. If a
named skill is absent, perform the equivalent focused check directly; SHIFU has
no required dependency on another plugin.

- `test-driven-development` for new behavior and bug fixes
- `systematic-debugging` for bugs, failures, and unexpected behavior
- `verification-before-completion` before claiming success
- `writing-plans` / `executing-plans` for larger planned work
- native `review`, `health`, or `verification-before-completion` for the depth selected by the active gear

## Gear Selection

Choose the gear internally. Surface it only when it changes cost, timing, verification depth, or a decision the user must make.

| Gear | Use When | Codex Path |
| --- | --- | --- |
| G1 | Clear, reversible work with limited blast radius and cheap acceptance | Act directly, run focused verification, summarize |
| G2 | Moderate unknowns, multi-step acceptance, or a meaningful but local blast radius | Brief design, use TDD where applicable, verify affected surface, review once |
| G3 | Hard-to-reverse work, broad blast radius, unresolved requirements, or costly acceptance | Write/confirm a plan, isolate work if useful, execute in stages, review and verify thoroughly |

A gear changes planning, review, and verification depth. It never overrides a more specific Skill, safety rule, or platform instruction. When signals conflict, choose the highest-risk reasonable gear without upgrading solely because many files are present.

## Routing

- When SHIFU is active and research depth needs adaptation: route internally to `shifu-research`.
- Formal source-backed reports and rewrites: use `reportclaw-coordinator` only when ReportClaw is installed and the task calls for it. That optional route is a terminal handoff for the current task. Otherwise use a source-bounded native workflow.
- For review depth, select the applicable native `review`, `health`, or `verification-before-completion` Skill when present; otherwise inspect the diff, repository health, or completion evidence directly. Do not execute a second SHIFU checklist.
- UI/design work: follow Codex frontend instructions first; use SHIFU only to choose process depth.
- User says `G1`, `G2`, `G3`, `upgrade`, `downgrade`, or `skip process`: adjust optional process depth, never mandatory safety or evidence checks.

## Practical Rules

- Do not load every SHIFU source skill. Keep context small.
- Do not duplicate Codex skills that already exist.
- Delegate only when the user requests it or an applicable skill requires independent parallel work.
- Never claim tests, builds, screenshots, installs, or pushes succeeded without fresh evidence.
- If the repository is dirty, protect unrelated user changes and stage only intended files.

## Strategic Handoff

Follow the active Codex platform instructions for progress updates. SHIFU only decides when a changed decision boundary, material uncertainty, user-only authority, or irreversible consequence requires a decision request.

Do not promise rollback from a bare checkpoint reference. When recovery changes the decision, state the recovery mode (`resume`, `restore`, `branch`, `compensate`, or `none`), what it covers, what it excludes, known external effects, and whether it was verified. Omit this detail when it would not change the next action; reduce or drop the rule after three unhelpful uses.

## Source

This Codex adapter maps SHIFU's process-depth judgment to Codex-native skills. It does not emulate Claude Code hooks or replace platform safety and verification rules.
