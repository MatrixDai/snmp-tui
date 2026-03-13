# snmp-cat — Implementation Tasks

Ordered task breakdown for building snmp-cat. Each milestone groups related tasks; tasks within a milestone are listed in recommended implementation order. Dependencies on prior milestones are noted where relevant.

---

## Milestone 1: Project Scaffolding

- [x] **1.1** Create root `Cargo.toml` with workspace members: `crates/mib-parser`, `crates/snmp-client`, `crates/snmp-cat`
- [x] **1.2** Stub out each crate with `Cargo.toml` + `lib.rs` (or `main.rs` for snmp-cat)
- [x] **1.3** Define shared types used across crates — `Oid` (numeric OID vector), `MibObject` (name, oid, module, syntax, access, status, description, index clause), `OidTree` (arena-based `Vec<Node>` with index references) — either in `mib-parser` (re-exported) or a shared module
- [x] **1.4** Add a `justfile` or `Makefile` with targets: `build`, `test`, `clippy`, `fmt-check`
- [x] **1.5** Confirm `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` all pass on the empty workspace

---

## Milestone 2: MIB Parser (`crates/mib-parser`)

- [x] **2.1** Add `pest` and `pest_derive` dependencies; create `grammar.pest` with rules for: whitespace/comments, identifiers, OID value notation, IMPORTS block
- [x] **2.2** Parse MODULE-IDENTITY definitions (module name, last-updated, organization, contact, description, revision list)
- [x] **2.3** Parse OBJECT-TYPE definitions — SMIv2 style (SYNTAX, MAX-ACCESS, STATUS, DESCRIPTION, INDEX, DEFVAL, `::=` OID assignment)
- [x] **2.4** Parse SMIv1 OBJECT-TYPE compat (ACCESS instead of MAX-ACCESS, old-style status values `mandatory`/`optional`/`obsolete`)
- [x] **2.5** Parse OBJECT-IDENTITY definitions
- [x] **2.6** Parse TEXTUAL-CONVENTION definitions (DISPLAY-HINT, STATUS, DESCRIPTION, SYNTAX)
- [x] **2.7** Parse SEQUENCE definitions and associate with table/row OBJECT-TYPEs (INDEX clause, column list)
- [x] **2.8** Implement arena-based OID tree builder — take parsed definitions and insert into `OidTree`, resolving parent chains from OID assignments
- [x] **2.9** Implement IMPORTS resolution — collect IMPORTS from each file, resolve cross-module references when loading multiple MIBs, topological sort or multi-pass loading
- [x] **2.10** Public API: `load_mibs(paths: &[PathBuf]) -> Result<OidTree>` — load a set of MIB files, resolve imports, return unified tree
- [x] **2.11** Bundle standard RFC MIB files as embedded resources or in a `mibs/` data directory: SNMPv2-SMI, SNMPv2-TC, SNMPv2-CONF, SNMPv2-MIB, IF-MIB, IP-MIB, TCP-MIB, UDP-MIB, HOST-RESOURCES-MIB
- [x] **2.12** Unit tests: parse each bundled RFC MIB successfully, verify known OIDs land at correct tree positions (e.g., sysDescr = 1.3.6.1.2.1.1.1), verify IMPORTS resolution across SNMPv2-SMI → SNMPv2-MIB chain

---

## Milestone 3: SNMP Client (`crates/snmp-client`)

> Depends on: Milestone 1 (shared types)

- [x] **3.1** Add `snmp2` and `tokio` dependencies; define `SnmpConfig` struct (host, port, version, community, timeout, retries, v3 credentials)
- [x] **3.2** Implement async `SnmpSession` wrapper — connect/create session from `SnmpConfig`
- [x] **3.3** Implement GET operation — `get(oid: &Oid) -> Result<SnmpValue>`
- [x] **3.4** Implement GETNEXT operation — `get_next(oid: &Oid) -> Result<(Oid, SnmpValue)>`
- [x] **3.5** Implement GETBULK operation — `get_bulk(oid: &Oid, max_repetitions: u32) -> Result<Vec<(Oid, SnmpValue)>>`
- [x] **3.6** Implement WALK — iterative GETNEXT (v1) or GETBULK (v2c/v3) that yields results until OID leaves the requested subtree
- [x] **3.7** Implement SET operation — `set(oid: &Oid, value: SnmpValue) -> Result<()>` with typed value variants (Integer, OctetString, IpAddress, Counter, Gauge, TimeTicks, ObjectIdentifier)
- [x] **3.8** Implement SNMPv3 USM support — auth protocols (MD5, SHA), privacy protocols (DES, AES)
- [x] **3.9** Define channel types: `SnmpRequest` enum (Get, GetNext, GetBulk, Walk, Set) and `SnmpResponse` struct (operation type, oid, result/error) for use by the TUI event loop
- [x] **3.10** Implement background task runner — spawns on a tokio runtime, receives `SnmpRequest` via `mpsc::Receiver`, sends `SnmpResponse` back via `mpsc::Sender`
- [x] **3.11** Integration tests — test against a local `snmpd` or mock; verify GET/WALK/SET round-trips and error handling (timeout, noSuchObject, auth failure)

---

## Milestone 4: TUI Shell (`crates/snmp-cat`)

> Depends on: Milestone 1

- [ ] **4.1** Add dependencies: `ratatui`, `crossterm`, `clap`, `serde`, `toml`, `tokio`
- [ ] **4.2** Define CLI args with clap: `--mib-dir`, `--mib-file`, `--host`, `--port`, `--community`, `--snmp-version`, `--timeout`, `--retries`
- [ ] **4.3** Implement config file loading from `~/.config/snmp-cat/config.toml` — merge with CLI args (CLI takes precedence)
- [ ] **4.4** Terminal setup: enter alternate screen, enable raw mode; teardown on exit (including panic hook for clean restore)
- [ ] **4.5** Event loop skeleton: `crossterm::event::poll(250ms)` → convert key events to `Message` → `update(&mut app, msg)` → `view(&app, &mut frame)`
- [ ] **4.6** Define `App` struct (model): `focused: FocusedPanel`, `tree: TreeState`, `detail: DetailState`, `results: ResultsState`, `modal: Option<Modal>`, `connection: Option<DeviceConnection>`, `mib_store: OidTree`
- [ ] **4.7** Define `Message` enum and `FocusedPanel` enum per design doc
- [ ] **4.8** Render three-panel layout with ratatui constraints (title bar Length(1), main area with 30/70 horizontal split, right area 50/50 vertical split, status bar Length(1))
- [ ] **4.9** Implement focus cycling: Tab / Shift+Tab rotates `FocusedPanel`; focused panel border cyan, unfocused gray
- [ ] **4.10** Render placeholder text in all panels, title bar with app name, status bar with key hints
- [ ] **4.11** `q` key → `Message::Quit` → clean exit

---

## Milestone 5: MIB Tree Panel

> Depends on: Milestone 2 (OID tree), Milestone 4 (TUI shell)

- [ ] **5.1** Build `TreeState` — tracks: selected index, expanded set (`HashSet<NodeIndex>`), scroll offset, flattened visible-node list
- [ ] **5.2** Render tree widget from `OidTree` — iterate visible nodes, indent by depth, prefix with `▸` (collapsed, has children) / `▾` (expanded) / space (leaf); branch nodes display as `name(subid)`, leaf nodes display as just `name`
- [ ] **5.3** Keyboard navigation: `j`/`↓` move selection down, `k`/`↑` move selection up in flattened list
- [ ] **5.4** Expand/collapse: `Enter`/`l`/`→` expands or enters; `h`/`←` collapses or moves to parent
- [ ] **5.5** Scroll viewport when selection moves beyond visible area
- [ ] **5.6** `gg` jump to top, `G` jump to bottom (requires tracking `g` press state with timeout or next-key check)
- [ ] **5.7** Selection change emits update to detail panel (drives content of Object Detail)

---

## Milestone 6: Detail & Results Panels

> Depends on: Milestone 5 (tree selection drives detail)

- [ ] **6.1** Detail panel: render MIB object metadata for selected tree node — Name, OID (dotted numeric), Module, Syntax, Access (MAX-ACCESS), Status, Description
- [ ] **6.2** Detail panel: for table/sequence objects, additionally show INDEX clause and column list
- [ ] **6.3** Detail panel: scrollable description text (`j`/`k` when detail panel is focused)
- [ ] **6.4** Results panel: define `ResultEntry` struct (operation type, oid, target device, value/error, timestamp)
- [ ] **6.5** Results panel: render scrollable log widget — newest entries at bottom
- [ ] **6.6** Results panel: format entries per operation type (GET single value, WALK multiple values, SET confirmation/error)
- [ ] **6.7** Results panel: auto-scroll on new entries; manual scroll override when user scrolls up; `G` to jump to latest

---

## Milestone 7: SNMP Integration

> Depends on: Milestone 3 (SNMP client), Milestone 6 (results panel)

- [ ] **7.1** Wire mpsc channels: main thread holds `Sender<SnmpRequest>` + `Receiver<SnmpResponse>`; spawn tokio background task with the other halves
- [ ] **7.2** Non-blocking check for `SnmpResponse` each event loop iteration (try_recv); dispatch `Message::SnmpResponse` on receipt
- [ ] **7.3** `g` key (tree focused) → send `SnmpRequest::Get` for selected OID → display result in results panel
- [ ] **7.4** `n` key → send `SnmpRequest::GetNext` → display response
- [ ] **7.5** `w` key → send `SnmpRequest::Walk` → stream results into results panel as they arrive
- [ ] **7.6** Loading indicator in status bar while SNMP operation is in-flight
- [ ] **7.7** Error display in results panel: timeout, noSuchObject, noSuchInstance, auth failure, network errors

---

## Milestone 8: Modal Dialogs

> Depends on: Milestone 7 (SNMP operations wired)

- [ ] **8.1** Modal rendering infrastructure: `Clear` widget + centered `Block::bordered()` overlay; when `app.modal.is_some()`, all input routed to modal handler
- [ ] **8.2** Device connection modal (`c` key): form fields for Host, Port, Version (cycle v1/v2c/v3), Community; Tab between fields, Enter to connect, Esc to cancel
- [ ] **8.3** Device connection modal: when v3 selected, show additional fields — Username, Auth Protocol (MD5/SHA), Auth Pass, Privacy Protocol (DES/AES), Privacy Pass
- [ ] **8.4** On connect: create `SnmpSession` via background task, update `app.connection`, show device info in title bar (e.g., `192.168.1.1 v2c`) or show `[No device]`
- [ ] **8.5** SNMP SET modal (`s` key, tree focused): pre-fill OID, Name, Type from selected node (read-only); value input field; type-aware input (integer for INTEGER/Counter/Gauge/TimeTicks, text for OCTET STRING/DisplayString, formatted IP for IpAddress); auto-append `.0` for scalar objects
- [ ] **8.6** Search modal (`/` key): text input with fuzzy match across all MIB object names; live-updating result list as user types; Enter selects and navigates tree to matched node; Esc cancels

---

## Milestone 9: Polish & Release

> Depends on: all prior milestones

- [ ] **9.1** Config file persistence — save connection settings and MIB directories to `~/.config/snmp-cat/config.toml` on change
- [ ] **9.2** `y` key (results panel focused) → copy selected result value to system clipboard
- [ ] **9.3** `?` key → toggle help overlay showing all key bindings
- [ ] **9.4** Color theme and consistent styling across all panels and modals
- [ ] **9.5** Edge case handling: no MIBs loaded (show helpful message), no device connected (disable SNMP keys, show hint), large WALK results (limit or paginate)
- [ ] **9.6** README screenshot / demo GIF
- [ ] **9.7** License selection and LICENSE file

---

## Dependency Graph

```
M1 Scaffolding
├── M2 MIB Parser
├── M3 SNMP Client
└── M4 TUI Shell
    └── M5 Tree Panel (needs M2)
        └── M6 Detail & Results
            └── M7 SNMP Integration (needs M3)
                └── M8 Modals
                    └── M9 Polish
```

Milestones 2, 3, and 4 can proceed in parallel after Milestone 1 is complete. From Milestone 5 onward, the path is sequential.
