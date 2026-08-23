---
name: test-first
description: "Use for all new code — enforces TDD with RED-GREEN-REFACTOR cycle and rationalization prevention"
---

# SHIFU: Test First

> **Gears:** G1 (run existing tests), G2/G3 (full TDD). See `shifu:engage` for gear definitions.

## The Iron Law

```
TEST PROVES THE NEED, CODE FILLS THE NEED
```

Every line of production code must exist because a test demanded it. Wrote code first? Delete it, write the test, start over.

## Gear Adaptation

- **G1:** If the change is covered by existing tests, run those tests to verify. New behavior still needs a new test. If no tests exist for the changed code, write a minimal test even for G1.
- **G2/G3:** Full TDD cycle for every piece of new behavior.
- **User says "skip tests":** Obey. User instructions override this skill.

## The Cycle

**RED — Write one failing test**
- Clear name describing the behavior
- Tests one thing
- Uses real code, not mocks — except for external HTTP calls, database connections, and third-party services (use stubs/mocks for those)

**VERIFY RED (mandatory)** — Run the test. It MUST fail. Failure reason must be "feature missing", not typo/syntax.

**GREEN — Write minimal code to pass**
- Simplest thing that works
- No extra features, no "while I'm here" improvements
- No refactoring yet

**VERIFY GREEN (mandatory)** — Run the test. It MUST pass. All other tests still green.

**REFACTOR — Clean up**
- Remove duplication, improve names, extract helpers
- Tests stay green throughout
- No new behavior in this phase

**Repeat** for next behavior.

## Red Flags — STOP

If you catch yourself thinking any of these, STOP and return to RED:
- "Let me write the implementation first, then add tests"
- "This is too simple to need a test"
- "I'll test it manually"
- "Just this one time without a test"
- "The test passes on first run" (you skipped RED)
- Test reveals a deeper bug? Invoke `shifu:debug` before continuing the TDD cycle

## Rationalization Prevention

| Excuse | Reality |
|--------|---------|
| "Too simple to test" | Simple code breaks. The test takes 30 seconds. Write it. |
| "I'll test after" | Tests-after proves "what does this do?" Tests-first proves "what should this do?" Completely different. |
| "Already manually tested" | Manual testing has no record, can't re-run, misses edge cases. |
| "Deleting my code is wasteful" | Sunk cost fallacy. Unverified code is technical debt, not an asset. Delete it. |
| "TDD slows me down" | TDD is faster than debugging. You're trading 30s of test writing for 30min of debugging. |

## Testing Anti-Patterns — Never Do These

**Never test mock behavior.** If your test passes because a mock returns what you told it to return, you tested nothing. The test should break when the *real* component breaks.

**Never add test-only methods to production classes.** No `_testGetState()`, no `destroy()` that only tests call. Test through the public interface. If you can't, the design needs work.

**Never test implementation details.** Test WHAT it does, not HOW. Refactoring internal logic should not break tests. If it does, your tests are coupled to implementation.

**Gate function pattern:** Call the function under test first, THEN assert on the result. Never assert inside setup helpers or constructors.

**One assertion concept per test.** A test named `test_user_creation` that also checks email sending and logging is three tests pretending to be one.

| Anti-Pattern | Symptom | Fix |
|-------------|---------|-----|
| Testing mocks | Test passes but production breaks | Remove mock, test real component |
| Test-only methods | Production class has methods nothing calls | Test through public API |
| Testing implementation | Refactoring breaks tests | Assert on outputs, not internals |
| God tests | 10+ assertions in one test | Split into focused tests |

## Completion Checklist

Before marking test work as done:
- [ ] Every new function has a test
- [ ] Each test was observed failing first (RED)
- [ ] Failure was for the expected reason
- [ ] Minimal code was written per test (GREEN)
- [ ] All tests pass with clean output
- [ ] Edge cases and error paths covered
- [ ] Coverage meets project threshold (typically 80%+)

## Outcome Logging

At the end of the TDD cycle, log what happened so SHIFU can track whether TDD caught real bugs:

```bash
SCRIPT="${CLAUDE_PLUGIN_ROOT}/hooks/scripts/metrics.sh"
# N = number of RED-phase failures that caught a real bug
# (expected behavior is what you designed for — unexpected failures = real catches)
bash "$SCRIPT" log "test_first_outcome" "<gear>" "<N>"
```

- `N=0` → RED worked as expected, GREEN passed first try (baseline)
- `N>=1` → a test surprised you by failing for a reason you didn't predict (real signal)
