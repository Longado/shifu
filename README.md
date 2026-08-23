# SHIFU (师父)

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-1.5.0-green.svg)](.claude-plugin/plugin.json)

Adaptive discipline system for AI coding agents.

Scales process depth by task risk — focused verification for small changes and
deeper planning, testing, and review for high-impact work.

> **Project status:** maintained core. Experimental runtimes, personal research,
> and private operating data are intentionally outside this public repository.

## The Problem

AI coding agents are capable but undisciplined. They skip tests, claim "should work" without verification, and apply the same approach whether you're fixing a typo or building a new system.

Rigorous workflows help, but the appropriate amount of process depends on
blast radius, reversibility, uncertainty, and the cost of acceptance.

## The Solution: Gear System

SHIFU automatically detects task complexity and selects the right gear:

| Gear | When | Process |
|------|------|---------|
| **G1** (Light) | Typos, config, small bugs | Do it → verify → done |
| **G2** (Medium) | Multi-file features | Quick design → TDD → single review → done |
| **G3** (Heavy) | Architecture, unclear requirements | Full spec → plan → worktree → TDD → two-round review → done |

Users can override anytime: `G1`, `G2`, `G3`, `upgrade`, `downgrade`.

## Quick Start

1. Install (see below)
2. Start a new Claude Code session
3. Ask for something — SHIFU auto-selects the gear
4. Override with `G1`/`G2`/`G3` if you disagree

## Skills (15 total)

| Skill | Purpose |
|-------|---------|
| `shifu:engage` | Entry point — detects gear, routes to skills |
| `shifu:delegate` | Model tier routing — picks cheapest model (T1/T2/T3) that hits quality bar |
| `shifu:think-first` | Design + planning (G2: quick chat, G3: full spec + plan) |
| `shifu:test-first` | TDD enforcement with rationalization prevention |
| `shifu:debug` | 4-phase root cause investigation |
| `shifu:review` | Adaptive code review + verification |
| `shifu:finish` | Worktree management + branch completion |
| `shifu:orchestrate` | G3 parallel subagent coordination (5+ tasks) |
| `shifu:migrate` | Structured data migration (Excel/CSV to SQLite) |
| `shifu:context` | Gear-aware context window management |
| `shifu:campaign` | Cross-session state persistence |
| `shifu:research` | Gear-aware research methodology with contrarian review |
| `shifu:research-navigation` | Find and reuse existing research assets before starting new retrieval |
| `shifu:reflect` | Session-end self-reflection + memory proposals |
| `shifu:llm-contract` | 9-point pre-flight checklist for LLM calls (seam design, composition defense, drift engineering) |

**Gear × Tier philosophy**: gear picks *process depth*, tier picks *model cost*. A G3 task rarely uses T3 for every step — only design/architecture steps do. See `shifu:delegate`.

## Commands

| Command | What it does |
|---------|-------------|
| `/brainstorm` | Explore approaches before coding |
| `/plan` | Create detailed implementation plan |
| `/debug` | Force full 4-phase debugging |

## How Skills Work

1. **Session start:** The `hooks/session-start` hook injects `shifu:engage` into every session.
2. **Gear detection:** `shifu:engage` analyzes the request and selects G1/G2/G3.
3. **Skill invocation:** Other skills load on demand via Claude Code's `Skill` tool.
4. **Cross-references:** Skills reference each other using `shifu:<name>` syntax.

Skills are Markdown files that shape agent behavior — not executable code.

## Core Principles

- **Iron Laws** — one non-negotiable rule per skill (e.g., "TEST PROVES THE NEED, CODE FILLS THE NEED")
- **Verification before claims** — run the command, read the output, THEN say it passes
- **Rationalization prevention** — pre-built tables of common excuses and why they're wrong
- **User intent controls optional process** — users can change gears or reduce ceremony; safety and evidence rules still apply

## Install

Prerequisites: Bash, Python 3, and `jq`. The Codex adapter installer also
expects a standard Codex home directory or an explicit `CODEX_HOME`.

### Claude Code

```bash
claude plugin marketplace add Longado/shifu
claude plugin install shifu@shifu-marketplace
```

**Verify:** Start a new session and ask Claude to fix a typo. It should announce "G1" and go straight to work.

### Codex

Clone the repository, then install the native adapter:

```bash
git clone https://github.com/Longado/shifu.git
cd shifu
bash scripts/install-codex-adapter.sh
```

Codex does not run Claude Code hooks. The adapter therefore routes G1/G2/G3
judgment to Codex-native planning, testing, debugging, review, and verification
skills when available, and falls back to direct equivalent checks instead of
trying to emulate the hook runtime.

## Public repository boundary

This repository contains the reusable SHIFU core only:

- process-depth skills and commands;
- portable Claude Code hooks;
- the Codex adapter and installer;
- contract and hook tests.

It does not contain user workspaces, transcripts, credentials, customer data,
private memory stores, or experimental multi-agent runtimes. Runtime state is
written under the host platform's plugin-data directory and is not committed.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on adding skills and submitting PRs.

## Credits

Inspired by [Superpowers](https://github.com/obra/superpowers) by Jesse Vincent
and contributors. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the
attribution boundary.

## License

MIT
