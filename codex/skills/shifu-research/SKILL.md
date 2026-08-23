---
name: shifu-research
description: Use when shifu routes an active SHIFU task for research-depth and decision-output adaptation.
---

# SHIFU Research For Codex

Use only when `shifu` routes here. This internal role adjusts research depth and decision framing; it does not replace browsing policy, `last30days`, ReportClaw, or repo inspection.

## Gear Adaptation

- G1: quick lookup or local file scan; answer with the key facts and caveats.
- G2: compare 2-3 options, name tradeoffs, recommend one path.
- G3: gather primary sources or local evidence, separate facts from inference, produce a decision memo or plan.

## Codex Rules

- Follow the active Codex source, browsing, citation, and repository-inspection policies.
- Do not duplicate existing research assets. If a repo has `docs/research/NAVIGATION.md`, read it first.

## Output Shape

For strategic research, prefer:

- `关键判断`
- `底层逻辑`
- `需要研判的分歧`
- `下一步决策`

Label fact, guidance, opinion, and inference. Do not lead with test counts, install logs, commands, or technical activity. Keep those as supporting evidence and surface them only when they change the decision or explain a failure. If no decision boundary changed, say so briefly instead of manufacturing an update.

For coding follow-up, hand back to `shifu` once. A formal-report handoff to `reportclaw-coordinator` is terminal: do not resume G gear or any SHIFU role afterward.
