# snmp-tui Architecture

## Arena-based OID Tree

`OidTree` is a `Vec<Node>` using index references (arena allocation pattern). Shared between parser output and TUI display.

- `TreeState` tracks: selected index, expanded set (`HashSet<NodeIndex>`), scroll offset, flattened visible-node list
- Shared types: `Oid` (numeric OID vector), `MibObject` (name, oid, module, syntax, access, status, description, index clause)

## Async Model

- TUI runs on the **main thread**
- SNMP operations run on a **tokio runtime** via **mpsc channels** (request/response pattern)
- Channel types: `SnmpRequest` enum (Get, GetNext, GetBulk, Walk, Set) and `SnmpResponse` struct
- Background task runner spawns on tokio, receives requests via `mpsc::Receiver`, sends responses via `mpsc::Sender`
- Event loop: `crossterm::event::poll(250ms)` -> key events to `Message` -> `update(&mut app, msg)` -> `view(&app, &mut frame)`
- Non-blocking `try_recv` for SNMP responses each iteration

## TUI Layout

Three-panel layout with title and status bars:

```
+-----------------------------------------------+
| Title bar (centered name, right-aligned device)|  Length(1)
+---------------+-------------------------------+
|               |  Object Detail (MIB metadata) |  Percentage(50)
|  MIB Tree     +-------------------------------+
|  (collapsible)|  Query Results (scrollable)   |  Percentage(50)
|               |                               |
+---------------+-------------------------------+
| Status bar (key hints, loading indicator)      |  Length(1)
+-----------------------------------------------+
  Percentage(30)        Percentage(70)
```

- **MIB Tree**: Collapsible with `>` / `v` indicators. Nodes: `name(subid)`, leaves: just `name`
- **Object Detail**: Name, OID, Module, Syntax, Access, Status, Description. Tables show INDEX clause and columns
- **Query Results**: Scrollable log of SNMP operations. Auto-scroll on new entries
- **Focus**: Tab/Shift+Tab cycling. Focused border = cyan, unfocused = gray

## MIB Parser

- **pest** PEG parser with custom `grammar.pest`
- Handles **SMIv1** (ACCESS/STATUS: mandatory/optional/obsolete) and **SMIv2** (MAX-ACCESS/STATUS: current/deprecated/obsolete)
- Parses: MODULE-IDENTITY, OBJECT-TYPE, OBJECT-IDENTITY, TEXTUAL-CONVENTION, SEQUENCE, IMPORTS
- IMPORTS resolution: collects imports per file, resolves cross-module references
- Public API: `load_mibs(paths: &[PathBuf]) -> Result<OidTree>`
- Bundles standard RFC MIBs: SNMPv2-SMI, SNMPv2-TC, SNMPv2-CONF, SNMPv2-MIB, IF-MIB, IP-MIB, TCP-MIB, UDP-MIB, HOST-RESOURCES-MIB

## SNMP Client

- Wraps the `snmp2` crate
- `SnmpConfig`: host, port, version, community, timeout, retries, v3 credentials
- Operations: `get`, `get_next`, `get_bulk`, `walk` (iterative GETNEXT for v1, GETBULK for v2c/v3), `set`
- SET typed variants: Integer, OctetString, IpAddress, Counter, Gauge, TimeTicks, ObjectIdentifier
- SNMPv3 USM: auth (MD5, SHA), privacy (DES, AES)

## Modal Dialogs

Modals overlay main layout using `Clear` widget + `Block::bordered()` on a centered `Rect`. When `app.modal.is_some()`, input routes to modal handler.

1. **Device Connection** (`c`): Host, Port, Version (cycle v1/v2c/v3), Community. V3 shows extra fields: Username, Auth Protocol/Pass, Privacy Protocol/Pass
2. **SNMP SET** (`s`): Pre-fills OID/Name/Type from selected node. Value input with type-aware handling. Auto-appends `.0` for scalars
3. **Search** (`/`): Fuzzy match on MIB object names. Live-updating results. Enter navigates tree to match

## Key Bindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle focus between panels |
| `c` | Open device connection dialog |
| `/` | Search MIB tree |
| `?` | Toggle help overlay |
| `q` | Quit |
| `j`/`k`/arrows | Navigate tree |
| `Enter`/`l`/`->` | Expand node |
| `h`/`<-` | Collapse node |
| `Space` | GET selected OID |
| `w` | WALK selected OID |
| `n` | GETNEXT |
| `s` | SET (opens dialog) |
| `gg` / `G` | Jump to top / bottom |
| `y` (results) | Copy selected result to clipboard |

## App State

`App` struct: `focused: FocusedPanel`, `tree: TreeState`, `detail: DetailState`, `results: ResultsState`, `modal: Option<Modal>`, `connection: Option<DeviceConnection>`, `mib_store: OidTree`
