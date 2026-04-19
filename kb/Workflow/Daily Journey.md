---
source: llm
---
# Daily Journey

Session workflow cheat sheet for Runway.

## Morning

```
/focus          Show milestone, deadlines, build health
/sync           Pull latest, PRs, issues, team activity
/next           Pick the next task from the milestone
```

## Work

```
/dev            Start local dev environment
/fix <issue>    Fix GitHub issue → branch → implement → verify
/check          Lint + test + type check
/pr [title]     Push and create pull request
/wip            Save work-in-progress, push for device switch
```

## Evening

```
/done           End session — progress, changelog, observations
```

## Weekly (Monday)

```
/audit          Security, compliance, drift, milestones
```

## Ad hoc

```
/ticket <desc>  Create a GitHub issue
/review <pr>    Review a pull request
/deploy [target] Deploy to Cloud Run or GPU worker
/experiment     Hypothesis-driven development
/help           Show this cheat sheet
```

## Build commands

```bash
just build          # cargo build --release
just build-quick    # fast iteration
just check          # cargo check --workspace
just test           # cargo test --all-targets
just lint           # fmt + clippy
just fix-lint       # auto-fix
just dev-up         # start local runtime
just smoke-test     # verify health
just dev-down       # stop runtime
just deploy-cloud-run  # deploy runtime
```

## Key files

| File | Purpose |
|------|---------|
| `AGENTS.md` | Canonical agent entrypoint |
| `CLAUDE.md` | Claude-specific rules |
| `MILESTONES.md` | Current sprint deliverables |
| `kb/Home.md` | KB index |
| `justfile` | All build/deploy/workflow commands |
