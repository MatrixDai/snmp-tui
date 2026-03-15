use std::path::PathBuf;

use clap::Parser;
use serde::{Deserialize, Serialize};

/// Interactive TUI tool for SNMP MIB exploration and device inspection.
#[derive(Parser, Debug)]
#[command(name = "snmp-tui", version, about)]
pub struct CliArgs {
    /// Path to additional MIB directory
    #[arg(long = "mib-dir")]
    pub mib_dir: Option<PathBuf>,

    /// Path to a single MIB file to load
    #[arg(long = "mib-file")]
    pub mib_file: Option<PathBuf>,

    /// SNMP timeout in milliseconds [default: 5000]
    #[arg(long)]
    pub timeout: Option<u64>,

    /// SNMP retries [default: 1]
    #[arg(long)]
    pub retries: Option<u32>,

    /// Maximum number of WALK result entries before truncation [default: 5000]
    #[arg(long)]
    pub max_walk_entries: Option<usize>,

    /// Enable debug logging to /tmp/snmp-tui-debug.log
    #[arg(long)]
    pub debug: bool,
}

/// A saved connection entry in the config file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionEntry {
    pub alias: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_read_community")]
    pub read_community: String,
    #[serde(default = "default_write_community")]
    pub write_community: String,
    // SNMPv3 fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priv_protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priv_password: Option<String>,
}

fn default_port() -> u16 {
    161
}
fn default_version() -> String {
    "v2c".to_string()
}
fn default_read_community() -> String {
    "public".to_string()
}
fn default_write_community() -> String {
    "private".to_string()
}

impl ConnectionEntry {
    /// Convert to an `SnmpConfig` for the SNMP client.
    pub fn to_snmp_config(&self, timeout_ms: u64, retries: u32) -> snmp_client::SnmpConfig {
        let version = match self.version.as_str() {
            "1" | "v1" => snmp_client::SnmpVersion::V1,
            "3" | "v3" => snmp_client::SnmpVersion::V3,
            _ => snmp_client::SnmpVersion::V2c,
        };

        let v3_credentials = if version == snmp_client::SnmpVersion::V3 {
            let auth_protocol = self.auth_protocol.as_deref().and_then(|s| match s {
                "MD5" => Some(snmp_client::AuthProtocol::Md5),
                "SHA" => Some(snmp_client::AuthProtocol::Sha1),
                "SHA-224" => Some(snmp_client::AuthProtocol::Sha224),
                "SHA-256" => Some(snmp_client::AuthProtocol::Sha256),
                "SHA-384" => Some(snmp_client::AuthProtocol::Sha384),
                "SHA-512" => Some(snmp_client::AuthProtocol::Sha512),
                _ => None,
            });
            let priv_protocol = self.priv_protocol.as_deref().and_then(|s| match s {
                "DES" => Some(snmp_client::PrivProtocol::Des),
                "AES-128" => Some(snmp_client::PrivProtocol::Aes128),
                "AES-192" => Some(snmp_client::PrivProtocol::Aes192),
                "AES-256" => Some(snmp_client::PrivProtocol::Aes256),
                _ => None,
            });
            Some(snmp_client::V3Credentials {
                username: self.username.clone().unwrap_or_default(),
                auth_protocol,
                auth_password: if auth_protocol.is_some() {
                    self.auth_password.clone()
                } else {
                    None
                },
                priv_protocol,
                priv_password: if priv_protocol.is_some() {
                    self.priv_password.clone()
                } else {
                    None
                },
            })
        } else {
            None
        };

        snmp_client::SnmpConfig {
            host: self.host.clone(),
            port: self.port,
            version,
            read_community: self.read_community.clone(),
            write_community: self.write_community.clone(),
            timeout_ms,
            retries,
            v3_credentials,
        }
    }
}

/// Configuration loaded from / saved to `~/.snmp-tui/config.toml`.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct FileConfig {
    pub mib_dirs: Vec<PathBuf>,
    pub mib_files: Vec<PathBuf>,
    pub max_walk_entries: Option<usize>,
    pub timeout: Option<u64>,
    pub retries: Option<u32>,
    pub last_connection: Option<String>,
    #[serde(default)]
    pub connections: Vec<ConnectionEntry>,
}

/// Resolved application configuration (file + CLI merged).
#[derive(Debug)]
pub struct AppConfig {
    pub mib_dirs: Vec<PathBuf>,
    pub mib_files: Vec<PathBuf>,
    pub timeout_ms: u64,
    pub retries: u32,
    pub max_walk_entries: usize,
    pub debug: bool,
    pub last_connection: Option<String>,
    pub connections: Vec<ConnectionEntry>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mib_dirs: Vec::new(),
            mib_files: Vec::new(),
            timeout_ms: 5000,
            retries: 1,
            max_walk_entries: 5000,
            debug: false,
            last_connection: None,
            connections: Vec::new(),
        }
    }
}

/// Load config file from `~/.snmp-tui/config.toml` if it exists.
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

/// Merge file config with CLI args. CLI takes precedence for global settings.
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
        timeout_ms: cli.timeout.or(file.timeout).unwrap_or(5000),
        retries: cli.retries.or(file.retries).unwrap_or(1),
        max_walk_entries: cli
            .max_walk_entries
            .or(file.max_walk_entries)
            .unwrap_or(5000),
        debug: cli.debug,
        last_connection: file.last_connection,
        connections: file.connections,
    }
}

/// Save a connection entry to the config file.
///
/// If a connection with the same alias exists, it is updated in place.
/// Otherwise the entry is appended (max 20 connections; oldest dropped if full).
/// Also sets `last_connection` to this alias.
pub fn save_connection(entry: &ConnectionEntry) {
    let mut config = load_config_file();

    if let Some(existing) = config
        .connections
        .iter_mut()
        .find(|c| c.alias == entry.alias)
    {
        *existing = entry.clone();
    } else {
        // Enforce max 20
        while config.connections.len() >= 20 {
            config.connections.remove(0);
        }
        config.connections.push(entry.clone());
    }

    config.last_connection = Some(entry.alias.clone());
    save_config_file(&config);
}

/// Delete a connection by alias from the config file.
pub fn delete_connection(alias: &str) {
    let mut config = load_config_file();
    config.connections.retain(|c| c.alias != alias);
    if config.last_connection.as_deref() == Some(alias) {
        config.last_connection = None;
    }
    save_config_file(&config);
}

/// Save config to `~/.snmp-tui/config.toml`.
fn save_config_file(config: &FileConfig) {
    if let Some(dir) = dirs_path() {
        if !dir.exists() {
            let _ = std::fs::create_dir_all(&dir);
        }
        if let Some(path) = dirs_config_path()
            && let Ok(contents) = toml::to_string_pretty(config)
        {
            let _ = std::fs::write(path, contents);
        }
    }
}

fn dirs_config_path() -> Option<PathBuf> {
    dirs_path().map(|p| p.join("config.toml"))
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".snmp-tui"))
}
