# Reflective Runway

This is the canonical agent entrypoint — all agents (Claude, Codex, Gemini, or otherwise) start here. Long-form documentation lives in `kb/`.

## Philosophy

Runway is the distribution and infrastructure layer for [Converge](https://github.com/Reflective-Lab/converge). It owns binaries, containers, GPU workers, and deployment scripts. The Converge SDK stays pure; Runway handles the messy reality of shipping.

We use strongly typed languages that compile to native code. Rust for the system. No virtual machines. No garbage collectors in the hot path.

## The Knowledgebase

`kb/` is an Obsidian vault. It is THE documentation for this project.

- Humans open it in Obsidian.
- AI agents read it with file tools.
- When you learn something, update the kb.
- When architecture changes, the kb changes first.

**Do NOT read the entire kb on startup.** Lazy-load:

1. Read `kb/Home.md` only when you need to find something (it's the index).
2. Follow ONE wikilink to the specific page you need.
3. Read that page. If it links to something else you need, follow that link.
4. Never bulk-read `kb/` — treat it like documentation you look up, not a preamble you memorize.

## Crates

| Crate | Purpose |
|---|---|
| `converge-application` | The `converge` CLI/TUI binary |
| `converge-llm` | Local LLM inference (Burn, llama.cpp) |

Both are proprietary and unpublished. They depend on Converge SDK crates via path (`../converge/crates/...`).

## Build

```bash
just build          # cargo build --release
just build-quick    # cargo build --profile quick-release
just check          # cargo check --workspace
just test           # cargo test --all-targets
just lint           # cargo fmt --check && cargo clippy -- -D warnings
just fix-lint       # auto-fix lint issues
just dev-up         # start local runtime
just dev-down       # stop local runtime
just smoke-test     # verify health
just deploy-cloud-run  # deploy runtime to Cloud Run
just focus          # session opener — repo health + recent activity
just sync           # team sync — PRs, issues, recent commits
```

## Rules

These are not suggestions.

- No `unsafe` code. Ever.
- Use typed enums, not strings with semantics.
- `just lint` clean before considering work done.
- No feature flags. No backwards-compat shims. Change the code.
- No unnecessary abstractions. Three similar lines beat a premature helper.
- All deps use `workspace = true` — never inline versions in crate Cargo.tomls.
- Edition 2024, rust-version 1.94.
- Runway **consumes** converge crates, never contributes to the SDK.
- Never commit secrets, .env files, or credentials.
- Never push to main without confirmation.

## Architecture

Runway has two crates and two infrastructure layers:

```
crates/application  →  converge/{core, experience, provider, ...}   CLI/TUI
crates/llm          →  converge/{core, domain, provider, storage}   Local inference
docker/             Container definitions
ops/                Deployment scripts, GPU infra
```

The Converge SDK lives in `../converge/`. Both repos must be siblings under `~/dev/work/`.

## Workflows

Run `just focus` at session start. See `kb/Workflow/Daily Journey.md` for the full cheat sheet.

| Workflow | Purpose |
|---|---|
| `/focus` / `just focus` | Session opener — orient yourself |
| `/sync` / `just sync` | Team sync — PRs, issues, build health |
| `/next` | Pick the next task from the current milestone |
| `/dev` | Start local development environment |
| `/check` | Code quality — lint, check, test |
| `/fix` | Fix a GitHub issue by number |
| `/pr` | Create a pull request |
| `/ticket` | Create an issue |
| `/done` | End-of-session — what you moved, what's left |
| `/review` | Review a pull request |
| `/wip` | Save work-in-progress and push |
| `/deploy` | Deploy to target environment |
| `/audit` | Security, dependency, compliance audit |
| `/help` | Show available skills |

## Milestones

Read `MILESTONES.md` at the start of every session (when it exists). See `~/dev/work/EPIC.md` for the strategic context and `~/dev/work/MILESTONES.md` for the cross-project rollup.
