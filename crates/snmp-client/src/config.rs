/// SNMP protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnmpVersion {
    V1,
    V2c,
    V3,
}

impl std::fmt::Display for SnmpVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnmpVersion::V1 => write!(f, "v1"),
            SnmpVersion::V2c => write!(f, "v2c"),
            SnmpVersion::V3 => write!(f, "v3"),
        }
    }
}

/// SNMPv3 authentication protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthProtocol {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

/// SNMPv3 privacy (encryption) protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivProtocol {
    Des,
    Aes128,
    Aes192,
    Aes256,
}

/// SNMPv3 USM credentials.
#[derive(Debug, Clone)]
pub struct V3Credentials {
    pub username: String,
    pub auth_protocol: Option<AuthProtocol>,
    pub auth_password: Option<String>,
    pub priv_protocol: Option<PrivProtocol>,
    pub priv_password: Option<String>,
}

/// SNMP connection configuration.
#[derive(Debug, Clone)]
pub struct SnmpConfig {
    pub host: String,
    pub port: u16,
    pub version: SnmpVersion,
    pub read_community: String,
    pub write_community: String,
    pub timeout_ms: u64,
    pub retries: u32,
    pub v3_credentials: Option<V3Credentials>,
}

impl Default for SnmpConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 161,
            version: SnmpVersion::V2c,
            read_community: "public".to_string(),
            write_community: "private".to_string(),
            timeout_ms: 5000,
            retries: 1,
            v3_credentials: None,
        }
    }
}

impl SnmpConfig {
    /// Returns the destination address as "host:port".
    pub fn destination(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
