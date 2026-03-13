# snmp-cat

An interactive TUI tool for exploring SNMP MIB trees, querying, and configuring network devices.

## Features

- Browse MIB object trees interactively with keyboard navigation
- Load standard and vendor MIB files (SMIv1 and SMIv2)
- Query live devices using SNMP GET, GETNEXT, and WALK (uses GETBULK internally for v2c/v3)
- Set OID values on devices using SNMP SET (with type-aware input based on MIB SYNTAX)
- View object details: syntax, access, status, description, OID path
- Support for SNMP v1, v2c, and v3

## Screenshot

<!-- TODO: Add screenshot -->

## Installation

```bash
# Build from source
git clone https://github.com/MatrixDai/snmp-cat.git
cd snmp-cat
cargo build --release

# Binary will be at target/release/snmp-cat
```

## Usage

```bash
# Launch with default MIB directory
snmp-cat

# Load specific MIB files or directories
snmp-cat --mib-dir /usr/share/snmp/mibs
snmp-cat --mib-file /path/to/MY-MIB.txt

# Connect to a device on startup
snmp-cat --host 192.168.1.1 --community public --snmp-version 2c
```

### Config File

Configuration is stored at `~/.config/snmp-cat/config.toml`:

```toml
[mibs]
directories = ["/usr/share/snmp/mibs"]

[defaults]
community = "public"
version = "2c"
timeout = 5
retries = 1
```

## SNMP Version Support

| Version | Auth | Privacy | Status |
|---------|------|---------|--------|
| v1 | Community string | None | Supported |
| v2c | Community string | None | Supported |
| v3 | USM (MD5/SHA) | DES/AES | Supported |

## MIB Loading

snmp-cat loads MIB files from configured directories at startup. It resolves IMPORTS dependencies automatically — load order does not matter as long as all referenced MIBs are available.

Standard RFC MIBs (SNMPv2-SMI, SNMPv2-TC, IF-MIB, etc.) are bundled with the application.

## Key Bindings

### Global

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus to next panel |
| `Shift+Tab` | Cycle focus to previous panel |
| `c` | Open device connection dialog |
| `/` | Search MIB tree by name |
| `?` | Toggle help overlay |
| `q` | Quit |

### MIB Tree Panel

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

### Detail & Results Panels

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `G` | Jump to latest result (results panel) |
| `y` | Copy selected result to clipboard (results panel) |

## License

<!-- TODO: Choose license -->
