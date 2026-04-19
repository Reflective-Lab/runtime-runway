---
name: done
description: End session — progress, changelog, observations.
user-invocable: true
allowed-tools: Read, Edit, Bash
---
# Done
End the session with accountability.
## Steps
1. Read `MILESTONES.md` — current milestone (if it exists).
2. Review session work: `git diff --stat HEAD && git log --oneline -5`
3. Check off completed deliverables in `MILESTONES.md` with today's date.
4. Update `kb/History/CHANGELOG.md` under `## [Unreleased]`. Skip trivial edits.
5. If the session changed project knowledge, architecture, or process, update the relevant `kb/` page.
6. Output:
```
── Done ───────────────────────────────────────────
Moved:
- <what was accomplished>
Remaining for <milestone> (<N> days left):
- <open deliverables>
Risks:
- <threats to deadline, or "None">
KB updates: <N pages updated, or "None">
────────────────────────────────────────────────────
```
## Rules
- Be honest. If nothing moved, say so.
- Partial work → don't check off, note progress.
- Work outside current milestone → flag as scope drift.
- Prefer updating existing `kb/` pages over creating new ad hoc notes.
