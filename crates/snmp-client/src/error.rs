use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnmpError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("timeout after {0}ms")]
    Timeout(u64),

    #[error("SNMP protocol error: {0}")]
    Protocol(String),

    #[error("SNMP error status {code}: {message}")]
    ErrorStatus { code: u32, message: String },

    #[error("authentication failure: {0}")]
    AuthFailure(String),

    #[error("no such object")]
    NoSuchObject,

    #[error("no such instance")]
    NoSuchInstance,

    #[error("end of MIB view")]
    EndOfMibView,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl From<snmp2::Error> for SnmpError {
    fn from(e: snmp2::Error) -> Self {
        match e {
            snmp2::Error::Send | snmp2::Error::Receive => SnmpError::Connection(format!("{:?}", e)),
            snmp2::Error::AuthFailure(_) => SnmpError::AuthFailure(format!("{:?}", e)),
            _ => SnmpError::Protocol(format!("{:?}", e)),
        }
    }
}

/// Map SNMP error status codes to human-readable messages.
pub fn error_status_message(code: u32) -> &'static str {
    match code {
        0 => "noError",
        1 => "tooBig",
        2 => "noSuchName",
        3 => "badValue",
        4 => "readOnly",
        5 => "genErr",
        6 => "noAccess",
        7 => "wrongType",
        8 => "wrongLength",
        9 => "wrongEncoding",
        10 => "wrongValue",
        11 => "noCreation",
        12 => "inconsistentValue",
        13 => "resourceUnavailable",
        14 => "commitFailed",
        15 => "undoFailed",
        16 => "authorizationError",
        17 => "notWritable",
        18 => "inconsistentName",
        _ => "unknownError",
    }
}
