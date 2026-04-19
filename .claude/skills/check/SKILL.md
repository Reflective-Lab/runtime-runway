---
name: check
description: Lint + type check + test. Am I clean?
user-invocable: true
allowed-tools: Bash, Read
---
# Check
Run the full quality gate for this project.
## Steps
1. `just check`
2. `just test`
3. `just lint`
4. Report failures with file paths and line numbers when available.
## Rules
- Fix auto-fixable issues (formatting, simple clippy).
- Report remaining issues clearly.
- If everything passes, just say "Clean."
