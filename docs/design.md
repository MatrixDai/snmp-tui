# snmp-tui — High-Level Design

This document describes the design of snmp-tui's TUI interface and application architecture. For features and usage, see [README.md](../README.md).

---

## 1. TUI Layout

Three-panel layout with title bar and status bar:

```
┌─────────────────────────────── snmp-tui ────────────────────────────────┐
│ MIB Tree              │ Object Detail                                   │
│                       │                                                 │
│ ▸ iso(1)              │  Name:     sysDescr                             │
│   ▸ org(3)            │  OID:      1.3.6.1.2.1.1.1                     │
│     ▸ dod(6)          │  Module:   SNMPv2-MIB                          │
│       ▸ internet(1)   │  Syntax:   DisplayString (SIZE (0..255))       │
│         ▸ mgmt(2)     │  Access:   read-only                           │
│           ▸ mib-2(1)  │  Status:   current                             │
│             ▾ system(1)│  Descr:   A textual description of the entity │
│               sysDescr │                                                │
│               sysObjID │                                                │
│               sysUpTime│                                                │
│               sysContact├────────────────────────────────────────────────┤
│               sysName  │ Query Results                                  │
│               sysLoc.. │                                                │
│                        │  GET 1.3.6.1.2.1.1.1.0 @ 192.168.1.1         │
│                        │  → "Cisco IOS Software, C2960 ..."            │
│                        │                                                │
│                        │  WALK 1.3.6.1.2.1.1 @ 192.168.1.1            │
│                        │  → .1.1.0 = "Cisco IOS Software..."           │
│                        │  → .1.2.0 = 1.3.6.1.4.1.9.1.716             │
│                        │  → .1.3.0 = 4823100                           │
├────────────────────────┴─────────────────────────────────────────────────┤
│ [Tab] Switch Panel  [g] GET  [w] WALK  [s] SET  [/] Search  [q] Quit   │
└──────────────────────────────────────────────────────────────────────────┘
```

### Layout Structure (ratatui constraints)

```
Vertical (outer)
├── Title bar           Length(1)
├── Main area           Min(0)
│   Horizontal
│   ├── MIB Tree        Percentage(30)
│   └── Right area      Percentage(70)
│       Vertical
│       ├── Object Detail    Percentage(50)
│       └── Query Results    Percentage(50)
└── Status bar          Length(1)
```

### Panels

**MIB Tree (left panel)**
- Collapsible tree widget with `▸`/`▾` indicators
- Navigate with `j`/`k` or arrow keys, expand/collapse with `Enter`
- Nodes display as `name(subid)` — e.g., `system(1)`, `sysDescr(1)`
- Leaf nodes (scalar objects) show just the name
- Table entries show `tableName` → `entryName` → column objects

**Object Detail (top-right panel)**
- Displays MIB metadata for the currently selected tree node
- Fields: Name, OID (dotted numeric), Module, Syntax, Access (MAX-ACCESS), Status, Description
- Description text wraps within the panel
- For table/sequence objects, additionally shows INDEX clause and column list

**Query Results (bottom-right panel)**
- Scrollable log of SNMP operations and their responses
- Each entry shows: operation type, OID, target device, and result value(s)
- Newest entries at the bottom, auto-scroll on new results
- Error responses displayed inline (e.g., `noSuchObject`, `noSuchInstance`, `timeout`)

**Title Bar**
- Centered app name: `snmp-tui`
- Right-aligned: connected device info (e.g., `192.168.1.1 v2c`) or `[No device]`

**Status Bar**
- Context-sensitive key hints based on focused panel and app state
- Shows loading indicator during active SNMP operations

---

## 2. Modal Dialogs

Modals overlay the main layout, capturing all input until dismissed. Rendered using `Clear` widget + `Block::bordered()` on a centered `Rect`.

### Device Connection (`c`)

```
┌──── Device Connection ────┐
│ Host:      [192.168.1.1 ] │
│ Port:      [161         ] │
│ Version:   [v2c ▾       ] │
│ Community: [public      ] │
│                           │
│    [Connect]  [Cancel]    │
└───────────────────────────┘
```

- Tab between fields, Enter to confirm, Esc to cancel
- Version field cycles through: v1 → v2c → v3
- When v3 selected, additional fields appear: Username, Auth Protocol, Auth Pass, Privacy Protocol, Privacy Pass

### SNMP SET (`s`)

```
┌────── SNMP SET ──────────────┐
│ OID:   1.3.6.1.2.1.1.5.0    │
│ Name:  sysName               │
│ Type:  DisplayString         │
│                              │
│ Value: [new-hostname       ] │
│                              │
│    [Send SET]  [Cancel]      │
└──────────────────────────────┘
```

- OID, Name, and Type are pre-filled from the selected tree node (read-only)
- Value input is type-aware:
  - `INTEGER` / enum: numeric input or dropdown of named values
  - `OCTET STRING` / `DisplayString`: text input
  - `IpAddress`: formatted IP input
  - `Counter`, `Gauge`, `TimeTicks`: numeric input
- `.0` instance suffix auto-appended for scalar objects

### Search (`/`)

```
┌──── Search MIB Tree ────┐
│ > sysN█                  │
│   sysName                │
│   sysNumConsoles         │
│   sysNotifications       │
└──────────────────────────┘
```

- Fuzzy search across all loaded MIB object names
- Results update as you type
- Enter selects and navigates tree to the matched node
- Esc cancels

---

## 3. Focus & Navigation

```rust
enum FocusedPanel {
    Tree,
    Detail,
    Results,
}
```

- `Tab` cycles: Tree → Detail → Results → Tree
- `Shift+Tab` cycles in reverse
- Focused panel border: highlighted color (cyan); unfocused: dim (gray)
- When a modal is open, panel focus is suspended — all input goes to the modal

---

## 4. Application Architecture (TEA)

The Elm Architecture — Model, Update, View:

```
                    ┌─────────┐
     Event ──────►  │ Update  │ ──────► new Model
                    └─────────┘
                         │
                    ┌─────────┐
     Model ──────►  │  View   │ ──────► Frame (rendered)
                    └─────────┘
```

### App State (Model)

```rust
struct App {
    focused: FocusedPanel,
    tree: TreeState,           // selected node, expanded set, scroll offset
    detail: DetailState,       // scroll offset for description
    results: ResultsState,     // log entries, scroll offset
    modal: Option<Modal>,      // active modal dialog
    connection: Option<DeviceConnection>,  // target device
    oid_tree: OidTree,          // arena-based OID tree (from mib-parser)
}
```

### Messages (Update)

```rust
enum Message {
    // Navigation
    FocusNext,
    FocusPrev,
    // Tree
    TreeUp,
    TreeDown,
    TreeExpand,
    TreeCollapse,
    // SNMP operations
    SnmpGet(Oid),
    SnmpGetNext(Oid),
    SnmpWalk(Oid),
    SnmpSet(Oid, Value),
    SnmpResponse(SnmpResult),
    // Modals
    OpenModal(ModalKind),
    CloseModal,
    ModalInput(ModalMessage),
    // Search
    SearchInput(char),
    SearchSelect,
    // App
    Tick,
    Quit,
}
```

---

## 5. Event Loop & Async I/O

```
┌──────────────┐       mpsc        ┌──────────────────┐
│  Main Thread │  SnmpRequest ───► │  Tokio Runtime    │
│              │                   │  (background)     │
│  crossterm   │  ◄─── SnmpResponse│                   │
│  event poll  │                   │  snmp2 client     │
│  update()    │                   │  GET/WALK/SET     │
│  view()      │                   │                   │
└──────────────┘                   └──────────────────┘
```

- Main thread: poll crossterm events (250ms timeout) → convert to `Message` → `update()` → `view()`
- Non-blocking check for `SnmpResponse` each loop iteration
- SNMP requests sent via `mpsc::Sender<SnmpRequest>` — does not block the UI
- Background tokio task receives requests, executes via `snmp2`, sends results back

---

## 6. Key Bindings

### Global (always active)

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus to next panel |
| `Shift+Tab` | Cycle focus to previous panel |
| `c` | Open device connection dialog |
| `/` | Open search dialog |
| `q` | Quit application |
| `?` | Toggle help overlay |

### MIB Tree Panel (when focused)

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `Enter` / `l` / `→` | Expand node / enter subtree |
| `h` / `←` | Collapse node / go to parent |
| `g` | SNMP GET on selected OID |
| `w` | SNMP WALK from selected OID |
| `n` | SNMP GETNEXT on selected OID |
| `s` | SNMP SET on selected OID (opens modal) |
| `G` | Jump to bottom of tree |
| `gg` | Jump to top of tree |

### Detail Panel (when focused)

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |

### Results Panel (when focused)

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `G` | Jump to latest result |
| `y` | Copy selected result value to clipboard |

### Modal Dialogs

| Key | Action |
|-----|--------|
| `Tab` | Next field |
| `Shift+Tab` | Previous field |
| `Enter` | Confirm / submit |
| `Esc` | Cancel and close |
