# Specs

Spec-driven, TDD development. Every issue starts from a written spec here, then:

1. **Spec** — write/finalize `specs/NNN-<slug>.md` (number matches the issue order).
2. **Red** — commit failing tests encoding the spec's "Test cases".
3. **Green** — implement to pass; nothing beyond the spec's scope.
4. **Propagate** — as part of Definition of Done, write the next spec stub and open
   the next GitHub issue, so the work self-propagates.

## Spec template

```markdown
# NNN <Title>

## Goal / user story
## Command surface  (bb command + flags, exit codes)
## Bitbucket endpoint(s) + request/response shape
## Behavior & edge cases  (errors, empty, not-found, 401)
## Test cases  (the failing tests to write first)
## Out of scope
## Next: spec NNN+? — <pointer to the issue this unlocks>
```
