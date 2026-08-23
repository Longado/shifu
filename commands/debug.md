---
description: "Start systematic debugging — forces full 4-phase root cause investigation"
---

Activate `shifu:debug` with all four phases mandatory.

Force the full investigation process even for bugs that seem obvious. This command means the user wants disciplined debugging, not quick guesses.

Iron Law: UNDERSTAND BEFORE YOU FIX.

Follow all four phases in order:
1. Evidence — read errors, reproduce, trace data flow
2. Pattern — find working examples, compare
3. Hypothesis — one change at a time
4. Fix — failing test first, then implement

Do NOT skip phases. Do NOT propose fixes before completing Phase 1.
