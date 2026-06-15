---
name: Spec issue
about: A spec-driven, TDD work item
title: "#N <title>"
labels: needs-spec
---

**Epic:** #
**Spec:** `specs/NNN-<slug>.md`
**Depends on:** #
**Unlocks:** #

### Acceptance criteria
- [ ] ...

### Definition of Done
- [ ] Spec `specs/NNN-<slug>.md` written/finalized
- [ ] Failing tests committed, then green (TDD)
- [ ] `cargo fmt --check` + `cargo clippy -D warnings` clean
- [ ] Help text / docs updated
- [ ] **Next spec stub written + next issue opened** (spec-driven loop)
