---
name: snmp-tui-dev
description: Assists with development of snmp-tui, an interactive Rust TUI tool for exploring SNMP MIB trees, querying, and configuring network devices. ALWAYS use this skill when the working directory is the snmp-tui repository — detectable by a Cargo workspace whose members include crates/mib-parser, crates/snmp-client, and crates/snmp-tui, or when a CLAUDE.md in the project root references snmp-tui. Also trigger when the user mentions snmp-tui, SNMP TUI, MIB parser, MIB browser, snmp-client crate, OID tree, mib-parser crate, ratatui SNMP, MIB explorer, or is working on Rust code that involves SNMP protocol operations, MIB file parsing (pest grammar, SMIv1/SMIv2), or TUI development with ratatui in this project context.
user-invocable: true
allowed-tools: Read, Grep, Glob, Bash(cargo *), Bash(gh *), Bash(git *), Bash(make *), Bash(ls *), Bash(wc *), Bash(diff *)
---

# snmp-tui Development Context

**snmp-tui** is an interactive terminal UI for exploring SNMP MIB trees, querying devices via SNMP (v1/v2c/v3), and performing SET operations. Built in Rust using ratatui + crossterm for the TUI and tokio for async SNMP operations.

## Workspace Structure

Cargo workspace (edition 2024) with 3 crates under `crates/`:

| Crate | Type | Role |
|-------|------|------|
| `mib-parser` | Library | pest-based SMIv1/SMIv2 MIB file parser; builds an arena-based OID tree |
| `snmp-client` | Library | Async SNMP v1/v2c/v3 wrapper around `snmp2`; GET/GETNEXT/GETBULK/WALK/SET |
| `snmp-tui` | Binary | TUI application — app state, event handling, config, modal dialogs |

## AI Notes

- **Local clone**: use the current working directory
- **Build**: `cargo build` or `make build`
- **Test**: `cargo test` or `make test` (use `-p <crate>` for a single crate)
- **Lint**: `cargo clippy -- -D warnings` or `make clippy`
- **Format check**: `cargo fmt --check` or `make fmt-check`
- **Config file**: `~/.snmp-tui/config.toml`
- For detailed architecture, see [architecture.md](architecture.md)

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `ratatui` | 0.30 | TUI framework |
| `crossterm` | 0.28 | Terminal backend |
| `snmp2` | 0.5 | SNMP v1/v2c/v3 protocol (features: `v3`, `heap_buffers`) |
| `pest` / `pest_derive` | 2 | PEG parser generator for MIB grammar |
| `tokio` | 1 | Async runtime (`rt-multi-thread`, `macros`, `sync`, `time`) |
| `clap` | 4 | CLI argument parsing (`derive` feature) |
| `serde` / `toml` | 1 / 0.8 | Config serialization |
| `thiserror` | 2 | Error types |

## CLI Arguments

`--mib-dir`, `--mib-file`, `--timeout`, `--retries`, `--max-walk-entries`, `--debug`

Host, port, community string, and SNMP version are configured via the TUI or `~/.snmp-tui/config.toml`, not CLI flags.
