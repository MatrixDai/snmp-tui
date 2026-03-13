use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnmpError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("timeout after {0}ms")]
    Timeout(u64),

    #[error("SNMP error: {0}")]
    Protocol(String),
}
