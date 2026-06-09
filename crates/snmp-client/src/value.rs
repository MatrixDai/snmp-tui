use std::fmt;

use mib_parser::Oid;

/// An SNMP value returned from or sent to an SNMP agent.
#[derive(Debug, Clone, PartialEq)]
pub enum SnmpValue {
    Integer(i64),
    OctetString(Vec<u8>),
    ObjectIdentifier(Oid),
    IpAddress([u8; 4]),
    Counter32(u32),
    Gauge32(u32),
    TimeTicks(u32),
    Counter64(u64),
    Opaque(Vec<u8>),
    Null,
    NoSuchObject,
    NoSuchInstance,
    EndOfMibView,
}

impl SnmpValue {
    /// Return the SNMP type label for display (e.g., "INTEGER", "STRING", "Timeticks").
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Integer(_) => "INTEGER",
            Self::OctetString(_) => "STRING",
            Self::ObjectIdentifier(_) => "OID",
            Self::IpAddress(_) => "IpAddress",
            Self::Counter32(_) => "Counter32",
            Self::Gauge32(_) => "Gauge32",
            Self::TimeTicks(_) => "Timeticks",
            Self::Counter64(_) => "Counter64",
            Self::Opaque(_) => "Opaque",
            Self::Null => "NULL",
            Self::NoSuchObject => "noSuchObject",
            Self::NoSuchInstance => "noSuchInstance",
            Self::EndOfMibView => "endOfMibView",
        }
    }
}

/// Format a byte slice as colon-separated lowercase hex (e.g. `de:ad:be:ef`).
fn format_hex_bytes(bytes: &[u8]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    hex.join(":")
}

impl fmt::Display for SnmpValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnmpValue::Integer(v) => write!(f, "{}", v),
            SnmpValue::OctetString(bytes) => match std::str::from_utf8(bytes) {
                Ok(s) => write!(f, "{}", s),
                Err(_) => write!(f, "{}", format_hex_bytes(bytes)),
            },
            SnmpValue::ObjectIdentifier(oid) => write!(f, "{}", oid),
            SnmpValue::IpAddress(addr) => {
                write!(f, "{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3])
            }
            SnmpValue::Counter32(v) => write!(f, "{}", v),
            SnmpValue::Gauge32(v) => write!(f, "{}", v),
            SnmpValue::TimeTicks(v) => {
                let centiseconds = *v as u64;
                let seconds = centiseconds / 100;
                let days = seconds / 86400;
                let hours = (seconds % 86400) / 3600;
                let minutes = (seconds % 3600) / 60;
                let secs = seconds % 60;
                write!(f, "({}) {}d {}h {}m {}s", v, days, hours, minutes, secs)
            }
            SnmpValue::Counter64(v) => write!(f, "{}", v),
            SnmpValue::Opaque(bytes) => write!(f, "{}", format_hex_bytes(bytes)),
            SnmpValue::Null => write!(f, "NULL"),
            SnmpValue::NoSuchObject => write!(f, "noSuchObject"),
            SnmpValue::NoSuchInstance => write!(f, "noSuchInstance"),
            SnmpValue::EndOfMibView => write!(f, "endOfMibView"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_integer() {
        assert_eq!(SnmpValue::Integer(42).to_string(), "42");
        assert_eq!(SnmpValue::Integer(-1).to_string(), "-1");
    }

    #[test]
    fn display_octet_string_utf8() {
        let val = SnmpValue::OctetString(b"hello".to_vec());
        assert_eq!(val.to_string(), "hello");
    }

    #[test]
    fn display_octet_string_hex() {
        let val = SnmpValue::OctetString(vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(val.to_string(), "de:ad:be:ef");
    }

    #[test]
    fn display_ip_address() {
        let val = SnmpValue::IpAddress([192, 168, 1, 1]);
        assert_eq!(val.to_string(), "192.168.1.1");
    }

    #[test]
    fn display_timeticks() {
        // 123456 centiseconds = 1234.56 seconds = 0d 0h 20m 34s
        let val = SnmpValue::TimeTicks(123456);
        assert_eq!(val.to_string(), "(123456) 0d 0h 20m 34s");
    }

    #[test]
    fn display_oid() {
        let val = SnmpValue::ObjectIdentifier(Oid::new(vec![1, 3, 6, 1, 2, 1]));
        assert_eq!(val.to_string(), "1.3.6.1.2.1");
    }

    #[test]
    fn display_special_values() {
        assert_eq!(SnmpValue::Null.to_string(), "NULL");
        assert_eq!(SnmpValue::NoSuchObject.to_string(), "noSuchObject");
        assert_eq!(SnmpValue::NoSuchInstance.to_string(), "noSuchInstance");
        assert_eq!(SnmpValue::EndOfMibView.to_string(), "endOfMibView");
    }
}
