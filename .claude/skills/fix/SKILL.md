---
name: fix
description: Fix a GitHub issue — read, branch, implement, check, PR.
user-invocable: true
argument-hint: [issue-number]
allowed-tools: Bash, Read, Edit, Write, Grep, Glob
---
# Fix #$ARGUMENTS
## Steps
1. Read the issue: `gh issue view $ARGUMENTS`
2. Branch: `git checkout -b fix/$ARGUMENTS`
3. Explore relevant code.
4. Implement the minimum fix. Follow existing patterns.
5. Verify: `just check`, `just test`, and `just lint` unless you have a documented reason not to.
6. Summarize the change and any remaining risk.
7. Commit, push, and open a PR only if the user asks.
