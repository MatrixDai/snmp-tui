# snmp-tui

An interactive TUI tool for exploring SNMP MIB trees, querying, and configuring network devices.

## Features

- Browse MIB object trees interactively with keyboard navigation
- Load standard and vendor MIB files (SMIv1 and SMIv2)
- Query live devices using SNMP GET, GETNEXT, and WALK (uses GETBULK internally for v2c/v3)
- Set OID values on devices using SNMP SET (with type-aware input based on MIB SYNTAX)
- View object details: syntax, access, status, description, OID path
- Support for SNMP v1, v2c, and v3

## Installation

```bash
# Build from source
git clone https://github.com/MatrixDai/snmp-tui.git
cd snmp-tui
cargo build --release

# Binary will be at target/release/snmp-tui
```

## Usage

```bash
# Launch with default MIB directory
snmp-tui

# Load specific MIB files or directories
snmp-tui --mib-dir /usr/share/snmp/mibs
snmp-tui --mib-file /path/to/MY-MIB.txt

# Other options
snmp-tui --timeout 10000 --retries 2 --max-walk-entries 1000 --debug
```

Host, community string, and SNMP version are configured via the connection dialog (`c`) or `~/.snmp-tui/config.toml`.

### Config File

Configuration is stored at `~/.snmp-tui/config.toml`:

```toml
mib_dirs = ["/usr/share/snmp/mibs"]
timeout = 5000
retries = 1

[[connections]]
alias = "my-router"
host = "192.168.1.1"
port = 161
version = "v2c"
read_community = "public"
write_community = "private"
```

## SNMP Version Support

| Version | Auth | Privacy | Status |
|---------|------|---------|--------|
| v1 | Community string | None | Supported |
| v2c | Community string | None | Supported |
| v3 | USM (MD5/SHA/SHA-2xx) | DES/AES | Supported |

## MIB Loading

snmp-tui loads MIB files from configured directories at startup. It resolves IMPORTS dependencies automatically — load order does not matter as long as all referenced MIBs are available.

Standard RFC MIBs (SNMPv2-SMI, SNMPv2-TC, IF-MIB, etc.) are bundled with the application.

## Key Bindings

### Global

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus to next panel |
| `Shift+Tab` | Cycle focus to previous panel |
| `c` | Open device connection dialog |
| `m` | Open MIB manager |
| `/` | Search MIB tree by name |
| `?` | Toggle help overlay |
| `[` / `]` | Shrink / grow active panel |
| `Ctrl+k` | Clear results |
| `q` | Quit |

### MIB Tree Panel

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `Enter` / `l` / `→` | Expand node / enter subtree |
| `h` / `←` | Collapse node / go to parent |
| `Space` | SNMP GET on selected OID |
| `w` | SNMP WALK from selected OID |
| `n` | SNMP GETNEXT on selected OID |
| `t` | SNMP table query on selected OID |
| `s` | SNMP SET on selected OID (opens modal) |
| `y` | Copy selected OID to clipboard |
| `r` | Reset tree to root |
| `G` | Jump to bottom of tree |
| `gg` | Jump to top of tree |

### Detail & Results Panels

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `G` | Jump to bottom |
| `gg` | Jump to top |
| `/` | Search within panel |
| `n` / `N` | Next / previous search match |
| `y` | Copy selected entry to clipboard |
| `e` | Export results to file (results panel) |

## License

This project is licensed under the [MIT License](LICENSE).
