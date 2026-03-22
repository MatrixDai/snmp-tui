# Contributing to snmp-tui

Thank you for your interest in contributing!

## Getting Started

```bash
git clone https://github.com/MatrixDai/snmp-tui.git
cd snmp-tui
cargo build
cargo test
```

## Development Workflow

1. Fork the repository and create a branch for your change
2. Make your changes
3. Ensure all checks pass before submitting a PR:
   ```bash
   cargo test
   cargo clippy -- -D warnings
   cargo fmt --check
   ```
4. Open a pull request against `main`

## Project Structure

| Crate | Role |
|-------|------|
| `crates/mib-parser` | pest-based SMIv1/SMIv2 MIB file parser |
| `crates/snmp-client` | Async SNMP v1/v2c/v3 client |
| `crates/snmp-tui` | TUI binary (ratatui + crossterm) |

See [CLAUDE.md](CLAUDE.md) for architecture notes and key dependencies.

## Reporting Issues

Please open a GitHub issue with:
- Steps to reproduce
- Expected vs. actual behavior
- Rust version (`rustc --version`) and OS

## Code Style

- Follow standard Rust formatting (`cargo fmt`)
- No `unwrap()`/`expect()` in production code — use proper error handling
- Add doc comments to public API items
