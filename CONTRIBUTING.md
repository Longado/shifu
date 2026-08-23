# Contributing to SHIFU

## Adding a New Skill

1. Create `skills/<skill-name>/SKILL.md`
2. Add frontmatter: `name`, `description`
3. Include a Gear reference header (`> **Gears:** ...`)
4. Include an Iron Law or core rule
5. Include Red Flags and/or Rationalization Prevention table
6. Stay under 200 lines
7. Update `skills/engage/SKILL.md` Skill Routing table
8. Run `bash tests/run-tests.sh` — all tests must pass

## Modifying Existing Skills

- Run tests before AND after changes
- Keep files under 200 lines
- Maintain `shifu:<name>` cross-reference consistency
- Don't change Iron Laws without evidence from pressure testing

## Skill Design Principles

- **Token budget:** Every paragraph must earn its place. If Claude already knows it, don't repeat it.
- **One purpose per skill:** A skill that does two things should be two skills.
- **Gear adaptation required:** Every skill must specify behavior per gear (G1/G2/G3).
- **Iron Laws are non-negotiable:** One per skill, stated early, enforced throughout.

## Testing

```bash
bash tests/run-tests.sh
```

Tests verify:
- Skill files have correct frontmatter
- No files exceed 200 lines
- No placeholder content (TBD, TODO, FIXME)
- All `shifu:<name>` references resolve
- Session-start hook outputs valid JSON

## Commit Convention

- `feat:` — new skill or feature
- `fix:` — bug fix or content correction
- `docs:` — documentation only
- `test:` — test additions or fixes
- `refactor:` — restructuring without behavior change
- `chore:` — tooling, CI, version bumps

## Pull Requests

- One skill per PR preferred
- Tests must pass
- Describe the problem solved, not just what changed
- Fill in all sections of the PR template
