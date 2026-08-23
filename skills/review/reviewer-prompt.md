# Code Reviewer Prompt Template

Use this template when dispatching a code-reviewer subagent.

## Template

````
You are reviewing code changes for: {WHAT_WAS_IMPLEMENTED}

**Requirements:** {PLAN_OR_REQUIREMENTS}

**Changes to review:**
```bash
git diff {BASE_REF}..{HEAD_REF}
```

**Your job — check BOTH in one pass:**

### 1. Spec Compliance
- Does the code implement ALL requirements? List any gaps.
- Does the code add anything NOT in requirements? Flag extras.
- Are there misunderstandings of requirements?

### 2. Code Quality
- Architecture: SOLID principles, separation of concerns
- Patterns: follows existing codebase conventions
- Security: input validation, no hardcoded secrets, injection prevention
- Testing: adequate coverage, tests behavior not implementation
- File organization: one clear responsibility per file, focused and testable

### 3. Human Fit
- Does the change preserve natural human phrasing instead of exposing system
  commands, rigid ritual, or agent-internal labels to users?
- Is the process depth gear-adaptive, or did the change force G3 ceremony onto
  small G1/G2 work?
- Are metaphors and frameworks used as internal tools rather than user-required
  language?

**Report format:**

Strengths: [what was done well]

Issues:
- Critical: [must fix — security, data loss, crash]
- Important: [should fix — bugs, quality]
- Minor: [nice to have — style, naming]

Assessment: [Ready to proceed / Needs fixes / Blocked]
````

## Placeholders

- `{WHAT_WAS_IMPLEMENTED}` — Brief description of the work
- `{PLAN_OR_REQUIREMENTS}` — The spec or task requirements
- `{BASE_REF}` — Git ref before changes (e.g., `origin/main`, or a commit SHA)
- `{HEAD_REF}` — Git ref after changes (e.g., `HEAD`, or a commit SHA)
