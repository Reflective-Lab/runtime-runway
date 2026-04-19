---
name: sync
description: Pull latest, show PRs, issues, milestone progress, service health.
user-invocable: true
allowed-tools: Bash, Read, Grep
---
# Sync
Morning briefing — catch up on everything.
## Steps
1. Run `just sync`
2. Read `MILESTONES.md` for milestone progress and open deliverables if needed.
3. Summarize the key blockers or changes for the user.
## Output
```
── Sync ───────────────────────────────────────────
PRs:       <N> open
Merged:    <N> since last sync
Issues:    <N> open
Milestone: <done>/<total>
Build:     <green|red>
────────────────────────────────────────────────────
```
## Rules
- Under 2 minutes. Brevity over completeness.
- Prefer the repo's `just sync` script over ad hoc git or GitHub commands.
