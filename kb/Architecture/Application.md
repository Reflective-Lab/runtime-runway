---
source: llm
---
# converge-application

The distribution binary for Converge. Packages domain packs, providers, and runtime into a deployable CLI/TUI product.

## Binary: `converge`

### Subcommands

| Command | Purpose |
|---------|---------|
| `tui` | Interactive terminal UI (ratatui + crossterm) |
| `packs` | Domain pack management (list, info) |
| `run` | Execute jobs from templates with JSON seeds |
| `eval` | Reproducible test fixtures |

### `run` options

- `--template` — job template name
- `--seed` — JSON input data
- `--json` — output cross-platform contract JSON
- `--mock` — use mock LLM backend
- `--budget` — cycle budget (default 50)
- Streaming output support

## Optional features

| Feature | What it enables |
|---------|-----------------|
| `tui` (default) | Terminal UI |
| `knowledge` | Vector knowledge base |
| `llm` | Local LLM inference via converge-llm |
| `analytics` | ML/analytics agents |
| `optimization` | OR-Tools constraint solver |
| `full` | All of the above |

## Dependencies

Required: `converge-core`, `converge-experience`, `converge-provider`

Note: domain agents are not registered by default. The binary warns:
> "No domain agents registered. Use organism-application for domain-specific packs."

## License

Proprietary (`LicenseRef-Proprietary`), not published to crates.io.

See also: [[Building/Deployment]], [[Architecture/Crate Map]]
