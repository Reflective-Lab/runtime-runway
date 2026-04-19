---
name: dev
description: Start local development environment.
user-invocable: true
argument-hint: [native|docker]
allowed-tools: Bash, Read
---
# Dev
Start local dev environment.
## Recipes
- `just dev-up` — start local runtime (native or Docker)
- `just smoke-test` — verify the local runtime
- `just dev-down` — stop the local runtime
- `just docker-up` — start Docker compose stack
- `just docker-up-extras` — start with NATS + SurrealDB
- `just docker-down` — stop Docker compose
- `cargo run -p converge-application` — run CLI directly
## Rules
- Check required tools are installed (rust, just, docker).
- Report missing dependencies clearly.
