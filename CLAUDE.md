# snmp-cat

Interactive TUI tool for SNMP MIB exploration and device inspection, written in Rust.

## Build / Test / Lint

```bash
cargo build                # build all crates
cargo test                 # run all tests
cargo test -p mib-parser   # test single crate
cargo clippy -- -D warnings
cargo fmt --check
```

## Workspace Structure

Cargo workspace with 3 crates under `crates/`:

- **`mib-parser`** — pest-based SMIv1/SMIv2 MIB file parser; builds an arena-based OID tree from MIB files
- **`snmp-client`** — async SNMP v1/v2c/v3 wrapper around the `snmp2` crate; GET/GETNEXT/GETBULK/WALK/SET operations
- **`snmp-cat`** — TUI binary (ratatui + crossterm); app state, event handling, config loading

## Architecture

- **OID tree**: arena-based (Vec<Node> with index references) — shared between parser output and TUI display
- **Async model**: TUI runs on main thread; SNMP operations run on tokio runtime via mpsc channels (request/response pattern)
- **TUI layout**: three panels — MIB tree browser (left), object detail view (top-right), query results (bottom-right)

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| ratatui 0.30+ | TUI framework |
| crossterm | Terminal backend |
| snmp2 | SNMP v1/v2c/v3 protocol |
| pest | PEG parser generator for MIB grammar |
| tokio | Async runtime for SNMP I/O |
| clap | CLI argument parsing |
| serde + toml | Config file handling |

## Workflow Rules

- **One task at a time**: Work on a single task from `docs/tasks.md` per iteration. Do not start the next task until the current one is complete and verified.
- **Branch per task**: Create a new branch for each task (e.g., `task/1.1-workspace-scaffolding`). Commit progress to the task branch after each iteration.
- **Test instructions required**: After completing each task, provide clear, copy-pasteable test instructions (commands to run, expected output, what to check) so the work can be verified before moving on.

## MIB Parser Notes

No existing pure-Rust MIB parser covers both SMIv1 and SMIv2 adequately. The `mib-parser` crate uses a custom pest grammar to handle:
- IMPORTS, OBJECT-TYPE, OBJECT-IDENTITY, MODULE-IDENTITY, TEXTUAL-CONVENTION
- SMIv1 `ACCESS`/`STATUS` syntax and SMIv2 `MAX-ACCESS`/`STATUS` syntax
- SEQUENCE definitions and table/row/column relationships
- Standard MIB distribution files (RFC MIBs, vendor MIBs)
