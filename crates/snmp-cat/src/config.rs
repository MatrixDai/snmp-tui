use std::path::PathBuf;

use clap::Parser;
use serde::Deserialize;

/// Interactive TUI tool for SNMP MIB exploration and device inspection.
#[derive(Parser, Debug)]
#[command(name = "snmp-cat", version, about)]
pub struct CliArgs {
    /// Path to additional MIB directory
    #[arg(long = "mib-dir")]
    pub mib_dir: Option<PathBuf>,

    /// Path to a single MIB file to load
    #[arg(long = "mib-file")]
    pub mib_file: Option<PathBuf>,

    /// SNMP target host
    #[arg(long)]
    pub host: Option<String>,

    /// SNMP target port
    #[arg(long)]
    pub port: Option<u16>,

    /// SNMP community string
    #[arg(long)]
    pub community: Option<String>,

    /// SNMP version (1, 2c, 3)
    #[arg(long = "snmp-version")]
    pub snmp_version: Option<String>,

    /// SNMP timeout in milliseconds
    #[arg(long)]
    pub timeout: Option<u64>,

    /// SNMP retries
    #[arg(long)]
    pub retries: Option<u32>,
}

/// Configuration loaded from `~/.config/snmp-cat/config.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub mib_dirs: Vec<PathBuf>,
    pub mib_files: Vec<PathBuf>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub community: Option<String>,
    pub snmp_version: Option<String>,
    pub timeout: Option<u64>,
    pub retries: Option<u32>,
}

/// Resolved application configuration (file + CLI merged).
#[derive(Debug)]
#[allow(dead_code)] // Fields used in later milestones
pub struct AppConfig {
    pub mib_dirs: Vec<PathBuf>,
    pub mib_files: Vec<PathBuf>,
    pub host: Option<String>,
    pub port: u16,
    pub community: String,
    pub snmp_version: String,
    pub timeout_ms: u64,
    pub retries: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mib_dirs: Vec::new(),
            mib_files: Vec::new(),
            host: None,
            port: 161,
            community: "public".to_string(),
            snmp_version: "v2c".to_string(),
            timeout_ms: 5000,
            retries: 1,
        }
    }
}

/// Load config file from `~/.config/snmp-cat/config.toml` if it exists.
pub fn load_config_file() -> FileConfig {
    if let Some(path) = dirs_config_path()
        && path.exists()
        && let Ok(contents) = std::fs::read_to_string(&path)
        && let Ok(config) = toml::from_str(&contents)
    {
        return config;
    }
    FileConfig::default()
}

/// Merge file config with CLI args. CLI takes precedence.
pub fn merge_config(file: FileConfig, cli: &CliArgs) -> AppConfig {
    let mut mib_dirs = file.mib_dirs;
    let mut mib_files = file.mib_files;

    if let Some(ref dir) = cli.mib_dir {
        mib_dirs.push(dir.clone());
    }
    if let Some(ref file_path) = cli.mib_file {
        mib_files.push(file_path.clone());
    }

    AppConfig {
        mib_dirs,
        mib_files,
        host: cli.host.clone().or(file.host),
        port: cli.port.unwrap_or(file.port.unwrap_or(161)),
        community: cli
            .community
            .clone()
            .or(file.community)
            .unwrap_or_else(|| "public".to_string()),
        snmp_version: cli
            .snmp_version
            .clone()
            .or(file.snmp_version)
            .unwrap_or_else(|| "v2c".to_string()),
        timeout_ms: cli.timeout.or(file.timeout).unwrap_or(5000),
        retries: cli.retries.or(file.retries).unwrap_or(1),
    }
}

/// Convert AppConfig to snmp_client::SnmpConfig for connecting to a device.
pub fn to_snmp_config(app_config: &AppConfig) -> Option<snmp_client::SnmpConfig> {
    let host = app_config.host.as_ref()?;

    let version = match app_config.snmp_version.as_str() {
        "1" | "v1" => snmp_client::SnmpVersion::V1,
        "3" | "v3" => snmp_client::SnmpVersion::V3,
        _ => snmp_client::SnmpVersion::V2c,
    };

    Some(snmp_client::SnmpConfig {
        host: host.clone(),
        port: app_config.port,
        version,
        community: app_config.community.clone(),
        timeout_ms: app_config.timeout_ms,
        retries: app_config.retries,
        v3_credentials: None,
    })
}

fn dirs_config_path() -> Option<PathBuf> {
    dirs_path().map(|p| p.join("config.toml"))
}

fn dirs_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config").join("snmp-cat"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("snmp-cat"))
    }
}
