use std::time::Instant;

use mib_parser::Oid;

use crate::config::SnmpConfig;
use crate::value::SnmpValue;

/// An SNMP request sent from the TUI to the background worker.
#[derive(Debug, Clone)]
pub enum SnmpRequest {
    /// Connect to a device with the given configuration.
    Connect(SnmpConfig),
    /// Disconnect the current session.
    Disconnect,
    /// GET a single OID value.
    Get(Oid),
    /// GETNEXT — get the next OID after the given one.
    GetNext(Oid),
    /// GETBULK — get multiple values starting from the given OID.
    GetBulk { oid: Oid, max_repetitions: u32 },
    /// WALK — iterate through the entire subtree rooted at the given OID.
    Walk(Oid),
    /// SET — write a value to the given OID.
    Set { oid: Oid, value: SnmpValue },
}

/// An SNMP response sent from the background worker to the TUI.
#[derive(Debug, Clone)]
pub struct SnmpResponse {
    /// The type of operation that generated this response.
    pub operation: OperationType,
    /// The OID that was queried (the request OID).
    pub request_oid: Oid,
    /// The result of the operation.
    pub result: SnmpResult,
    /// When the response was created.
    pub timestamp: Instant,
}

/// The type of SNMP operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Connect,
    Disconnect,
    Get,
    GetNext,
    GetBulk,
    Walk,
    Set,
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationType::Connect => write!(f, "CONNECT"),
            OperationType::Disconnect => write!(f, "DISCONNECT"),
            OperationType::Get => write!(f, "GET"),
            OperationType::GetNext => write!(f, "GETNEXT"),
            OperationType::GetBulk => write!(f, "GETBULK"),
            OperationType::Walk => write!(f, "WALK"),
            OperationType::Set => write!(f, "SET"),
        }
    }
}

/// The result of an SNMP operation.
#[derive(Debug, Clone)]
pub enum SnmpResult {
    /// A single value result.
    Value(Oid, SnmpValue),
    /// Multiple (OID, value) pairs (from WALK or GETBULK).
    MultiValue(Vec<(Oid, SnmpValue)>),
    /// Operation completed successfully with no value (e.g., SET, CONNECT).
    Ok(String),
    /// Operation failed with an error message.
    Error(String),
}

impl SnmpResponse {
    pub fn ok(operation: OperationType, request_oid: Oid, message: String) -> Self {
        Self {
            operation,
            request_oid,
            result: SnmpResult::Ok(message),
            timestamp: Instant::now(),
        }
    }

    pub fn error(operation: OperationType, request_oid: Oid, message: String) -> Self {
        Self {
            operation,
            request_oid,
            result: SnmpResult::Error(message),
            timestamp: Instant::now(),
        }
    }

    pub fn value(operation: OperationType, request_oid: Oid, oid: Oid, value: SnmpValue) -> Self {
        Self {
            operation,
            request_oid,
            result: SnmpResult::Value(oid, value),
            timestamp: Instant::now(),
        }
    }

    pub fn multi_value(
        operation: OperationType,
        request_oid: Oid,
        values: Vec<(Oid, SnmpValue)>,
    ) -> Self {
        Self {
            operation,
            request_oid,
            result: SnmpResult::MultiValue(values),
            timestamp: Instant::now(),
        }
    }
}
