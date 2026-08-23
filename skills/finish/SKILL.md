---
name: finish
description: "Use when work is complete — handles worktree setup (G3), branch completion, and cleanup"
---

# SHIFU: Finish

> **Gears:** G3 (worktree required), G2 (worktree optional), G1 (branch completion only). See `shifu:engage` for gear definitions.

Handles workspace isolation and branch completion in one skill.

## Worktree Setup (G3 start, optional for G2)

**When to create a worktree:**
- G3 tasks — always (before implementation begins)
- G2 tasks — if user requests isolation, or if changes are risky
- G1 tasks — never

**Creation steps:**
1. Check for existing worktree directory (`.worktrees/` or `worktrees/`)
2. Verify directory is in `.gitignore` — if not, add it and commit
3. Create worktree: `git worktree add <path> -b <branch-name>`
4. Run dependency install (auto-detect: check for package.json → npm install, Cargo.toml → cargo build, requirements.txt → pip install, go.mod → go mod download. If multiple found, run all.)
5. Run tests to verify clean baseline
6. Report: "Worktree ready at <path>. Tests passing (<N> tests). Ready to implement."

**If baseline tests fail:** Report failures, ask user whether to proceed or investigate.

## Branch Completion (all gears)

### Step 1: Verify Tests

```bash
# Run project's test suite
npm test / cargo test / pytest / go test ./...
```

**Tests fail?** Stop. Fix before proceeding. Do not present completion options.

### Step 2: Present Options

```
Work complete. What would you like to do?

1. Merge to <base-branch> locally
2. Push and create a Pull Request
3. Keep the branch as-is
4. Discard this work
```

### Step 3: Execute Choice

**Option 1 — Merge:**
```bash
git checkout <base>
git pull
git merge <feature>
# Run tests on merged result
git branch -d <feature>
```
Clean up worktree.

**Option 2 — Push + PR:**
```bash
git push -u origin <feature>
gh pr create --title "<title>" --body "<summary + test plan>"
```
Clean up worktree.

**Option 3 — Keep:** Report branch and worktree location. No cleanup.

**Option 4 — Discard:**
Require exact string `discard` (lowercase, no surrounding text) as confirmation. Then:
```bash
git checkout <base>
git branch -D <feature>
```
Clean up worktree.

### Step 4: Worktree Cleanup

For Options 1, 2, 4 — remove worktree if one exists:
```bash
git worktree remove <path>
```
If removal fails due to uncommitted changes, report the error and ask the user before using `--force`.

## Red Flags

- Proceeding with failing tests
- Merging without running tests on the merged result
- Deleting work without user confirmation
- Force-pushing without explicit user request
