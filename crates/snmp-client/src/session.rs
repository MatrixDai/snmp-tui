use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use mib_parser::Oid;

use crate::config::{AuthProtocol, PrivProtocol, SnmpConfig, SnmpVersion};
use crate::error::{SnmpError, error_status_message};
use crate::value::SnmpValue;

/// Resolve a host:port string to a SocketAddr, preferring IPv4.
///
/// Many SNMP agents only listen on IPv4, but `localhost` often resolves
/// to `::1` (IPv6) first on Linux. This causes silent send failures
/// and receive timeouts. By explicitly resolving and preferring IPv4,
/// we match the behavior of standard SNMP tools like snmpget.
fn resolve_prefer_ipv4(dest: &str) -> Result<SocketAddr, SnmpError> {
    let addrs: Vec<SocketAddr> = dest
        .to_socket_addrs()
        .map_err(|e| SnmpError::Connection(format!("DNS resolution failed for {}: {}", dest, e)))?
        .collect();

    if addrs.is_empty() {
        return Err(SnmpError::Connection(format!(
            "no addresses found for {}",
            dest
        )));
    }

    // Prefer IPv4
    addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(addrs.first())
        .copied()
        .ok_or_else(|| SnmpError::Connection(format!("no addresses found for {}", dest)))
}

/// Convert our Oid to snmp2's Oid.
fn to_snmp2_oid(oid: &Oid) -> Result<snmp2::Oid<'static>, SnmpError> {
    let components: Vec<u64> = oid.components().iter().map(|&c| c as u64).collect();
    snmp2::Oid::from(&components).map_err(|e| SnmpError::Protocol(format!("invalid OID: {:?}", e)))
}

/// Convert snmp2's Oid to our Oid.
fn from_snmp2_oid(oid: &snmp2::Oid<'_>) -> Oid {
    let components: Vec<u32> = oid
        .iter()
        .map(|iter| iter.map(|v| v as u32).collect())
        .unwrap_or_default();
    Oid::new(components)
}

/// Convert snmp2::Value to our SnmpValue.
fn from_snmp2_value(value: &snmp2::Value<'_>) -> SnmpValue {
    match value {
        snmp2::Value::Integer(v) => SnmpValue::Integer(*v),
        snmp2::Value::OctetString(bytes) => SnmpValue::OctetString(bytes.to_vec()),
        snmp2::Value::ObjectIdentifier(oid) => SnmpValue::ObjectIdentifier(from_snmp2_oid(oid)),
        snmp2::Value::IpAddress(addr) => SnmpValue::IpAddress(*addr),
        snmp2::Value::Counter32(v) => SnmpValue::Counter32(*v),
        snmp2::Value::Unsigned32(v) => SnmpValue::Gauge32(*v),
        snmp2::Value::Timeticks(v) => SnmpValue::TimeTicks(*v),
        snmp2::Value::Counter64(v) => SnmpValue::Counter64(*v),
        snmp2::Value::Opaque(bytes) => SnmpValue::Opaque(bytes.to_vec()),
        snmp2::Value::Null => SnmpValue::Null,
        snmp2::Value::NoSuchObject => SnmpValue::NoSuchObject,
        snmp2::Value::NoSuchInstance => SnmpValue::NoSuchInstance,
        snmp2::Value::EndOfMibView => SnmpValue::EndOfMibView,
        _ => SnmpValue::Null,
    }
}

/// Check PDU for SNMP error status and return error if present.
fn check_pdu_error(pdu: &snmp2::Pdu<'_>) -> Result<(), SnmpError> {
    if pdu.error_status != 0 {
        return Err(SnmpError::ErrorStatus {
            code: pdu.error_status,
            message: error_status_message(pdu.error_status).to_string(),
        });
    }
    Ok(())
}

/// Map our AuthProtocol to snmp2's.
fn to_snmp2_auth_protocol(proto: AuthProtocol) -> snmp2::v3::AuthProtocol {
    match proto {
        AuthProtocol::Md5 => snmp2::v3::AuthProtocol::Md5,
        AuthProtocol::Sha1 => snmp2::v3::AuthProtocol::Sha1,
        AuthProtocol::Sha224 => snmp2::v3::AuthProtocol::Sha224,
        AuthProtocol::Sha256 => snmp2::v3::AuthProtocol::Sha256,
        AuthProtocol::Sha384 => snmp2::v3::AuthProtocol::Sha384,
        AuthProtocol::Sha512 => snmp2::v3::AuthProtocol::Sha512,
    }
}

/// Map our PrivProtocol to snmp2 Cipher.
fn to_snmp2_cipher(proto: PrivProtocol) -> snmp2::v3::Cipher {
    match proto {
        PrivProtocol::Des => snmp2::v3::Cipher::Des,
        PrivProtocol::Aes128 => snmp2::v3::Cipher::Aes128,
        PrivProtocol::Aes192 => snmp2::v3::Cipher::Aes192,
        PrivProtocol::Aes256 => snmp2::v3::Cipher::Aes256,
    }
}

/// An SNMP session wrapping the snmp2 SyncSession.
///
/// This is NOT Send/Sync — it must be used within a single thread.
/// For async use, wrap calls with `tokio::task::spawn_blocking`.
pub struct SnmpSession {
    read_session: snmp2::SyncSession,
    write_session: Option<snmp2::SyncSession>,
    config: SnmpConfig,
}

impl std::fmt::Debug for SnmpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnmpSession")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl SnmpSession {
    /// Create a new SNMP session from configuration.
    ///
    /// For v1/v2c, creates separate read and write sessions using
    /// `read_community` and `write_community` respectively, so that
    /// SET operations use the correct write community string.
    pub fn new(config: SnmpConfig) -> Result<Self, SnmpError> {
        let dest = config.destination();
        let timeout = Duration::from_millis(config.timeout_ms);

        // Resolve hostname preferring IPv4 to avoid IPv6 issues
        // (many SNMP agents only listen on IPv4)
        let resolved = resolve_prefer_ipv4(&dest)?;

        let (read_session, write_session) = match config.version {
            SnmpVersion::V1 => {
                let read = snmp2::SyncSession::new_v1(
                    resolved,
                    config.read_community.as_bytes(),
                    Some(timeout),
                    0,
                )
                .map_err(SnmpError::Io)?;
                let write = snmp2::SyncSession::new_v1(
                    resolved,
                    config.write_community.as_bytes(),
                    Some(timeout),
                    0,
                )
                .map_err(SnmpError::Io)?;
                (read, Some(write))
            }
            SnmpVersion::V2c => {
                let read = snmp2::SyncSession::new_v2c(
                    resolved,
                    config.read_community.as_bytes(),
                    Some(timeout),
                    0,
                )
                .map_err(SnmpError::Io)?;
                let write = snmp2::SyncSession::new_v2c(
                    resolved,
                    config.write_community.as_bytes(),
                    Some(timeout),
                    0,
                )
                .map_err(SnmpError::Io)?;
                (read, Some(write))
            }
            SnmpVersion::V3 => {
                let creds = config
                    .v3_credentials
                    .as_ref()
                    .ok_or_else(|| SnmpError::AuthFailure("v3 credentials required".to_string()))?;

                let security = build_v3_security(creds);

                let mut session = snmp2::SyncSession::new_v3(resolved, Some(timeout), 0, security)
                    .map_err(SnmpError::Io)?;

                // Initialize v3 session (discovers engine ID)
                session.init().map_err(SnmpError::from)?;

                (session, None)
            }
        };

        Ok(Self {
            read_session,
            write_session,
            config,
        })
    }

    /// Get the session's configuration.
    pub fn config(&self) -> &SnmpConfig {
        &self.config
    }

    /// Perform a GET operation for a single OID.
    pub fn get(&mut self, oid: &Oid) -> Result<SnmpValue, SnmpError> {
        let snmp_oid = to_snmp2_oid(oid)?;

        let mut last_err = None;
        for _ in 0..=self.config.retries {
            match self.read_session.get(&snmp_oid) {
                Ok(mut pdu) => {
                    check_pdu_error(&pdu)?;
                    if let Some((_oid, value)) = pdu.varbinds.next() {
                        let val = from_snmp2_value(&value);
                        if matches!(val, SnmpValue::NoSuchObject) {
                            return Err(SnmpError::NoSuchObject);
                        }
                        if matches!(val, SnmpValue::NoSuchInstance) {
                            return Err(SnmpError::NoSuchInstance);
                        }
                        return Ok(val);
                    }
                    return Ok(SnmpValue::Null);
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .map(SnmpError::from)
            .unwrap_or_else(|| SnmpError::Protocol("no response received".to_string())))
    }

    /// Perform a GETNEXT operation.
    /// Returns the next OID and its value.
    pub fn get_next(&mut self, oid: &Oid) -> Result<(Oid, SnmpValue), SnmpError> {
        let snmp_oid = to_snmp2_oid(oid)?;

        let mut last_err = None;
        for _ in 0..=self.config.retries {
            match self.read_session.getnext(&snmp_oid) {
                Ok(mut pdu) => {
                    check_pdu_error(&pdu)?;
                    if let Some((resp_oid, value)) = pdu.varbinds.next() {
                        let val = from_snmp2_value(&value);
                        if matches!(val, SnmpValue::EndOfMibView) {
                            return Err(SnmpError::EndOfMibView);
                        }
                        return Ok((from_snmp2_oid(&resp_oid), val));
                    }
                    return Err(SnmpError::Protocol("empty response".to_string()));
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .map(SnmpError::from)
            .unwrap_or_else(|| SnmpError::Protocol("no response received".to_string())))
    }

    /// Perform a GETBULK operation (v2c/v3 only).
    /// Returns a list of (OID, value) pairs.
    pub fn get_bulk(
        &mut self,
        oid: &Oid,
        max_repetitions: u32,
    ) -> Result<Vec<(Oid, SnmpValue)>, SnmpError> {
        if self.config.version == SnmpVersion::V1 {
            return Err(SnmpError::Protocol(
                "GETBULK not supported in SNMPv1".to_string(),
            ));
        }

        let snmp_oid = to_snmp2_oid(oid)?;

        let mut last_err = None;
        for _ in 0..=self.config.retries {
            match self.read_session.getbulk(&[&snmp_oid], 0, max_repetitions) {
                Ok(pdu) => {
                    check_pdu_error(&pdu)?;
                    let results: Vec<(Oid, SnmpValue)> = pdu
                        .varbinds
                        .map(|(resp_oid, value)| {
                            (from_snmp2_oid(&resp_oid), from_snmp2_value(&value))
                        })
                        .collect();
                    return Ok(results);
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .map(SnmpError::from)
            .unwrap_or_else(|| SnmpError::Protocol("no response received".to_string())))
    }

    /// Perform a WALK operation — iterates through the subtree rooted at the given OID.
    /// Uses GETNEXT for v1, GETBULK for v2c/v3.
    /// Returns all (OID, value) pairs within the subtree.
    /// Stops after 10,000 entries to prevent unbounded memory growth.
    pub fn walk(&mut self, oid: &Oid) -> Result<Vec<(Oid, SnmpValue)>, SnmpError> {
        const MAX_WALK_ENTRIES: usize = 10_000;
        let root_components = oid.components().to_vec();
        let mut results = Vec::new();
        let mut current_oid = oid.clone();

        loop {
            if results.len() >= MAX_WALK_ENTRIES {
                break;
            }
            if self.config.version == SnmpVersion::V1 {
                match self.get_next(&current_oid) {
                    Ok((next_oid, value)) => {
                        if !next_oid.components().starts_with(&root_components) {
                            break;
                        }
                        current_oid = next_oid.clone();
                        results.push((next_oid, value));
                    }
                    Err(SnmpError::EndOfMibView) => break,
                    Err(e) => return Err(e),
                }
            } else {
                let bulk_results = self.get_bulk(&current_oid, 10)?;
                if bulk_results.is_empty() {
                    break;
                }

                let mut done = false;
                for (next_oid, value) in bulk_results {
                    if matches!(value, SnmpValue::EndOfMibView) {
                        done = true;
                        break;
                    }
                    if !next_oid.components().starts_with(&root_components) {
                        done = true;
                        break;
                    }
                    current_oid = next_oid.clone();
                    results.push((next_oid, value));
                }

                if done {
                    break;
                }
            }
        }

        Ok(results)
    }

    /// Perform a SET operation.
    /// Uses the write session (write_community) for v1/v2c, falls back to read session for v3.
    pub fn set(&mut self, oid: &Oid, value: &SnmpValue) -> Result<(), SnmpError> {
        let snmp_oid = to_snmp2_oid(oid)?;
        let session = self
            .write_session
            .as_mut()
            .unwrap_or(&mut self.read_session);

        let mut last_err = None;
        for _ in 0..=self.config.retries {
            match session.set(&[(&snmp_oid, to_snmp2_value(value))]) {
                Ok(pdu) => {
                    check_pdu_error(&pdu)?;
                    return Ok(());
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .map(SnmpError::from)
            .unwrap_or_else(|| SnmpError::Protocol("no response received".to_string())))
    }
}

/// Build snmp2 v3 Security from our credentials.
fn build_v3_security(creds: &crate::config::V3Credentials) -> snmp2::v3::Security {
    let auth_pass = creds.auth_password.as_deref().unwrap_or("").as_bytes();
    let mut security = snmp2::v3::Security::new(creds.username.as_bytes(), auth_pass);

    if let Some(auth_proto) = creds.auth_protocol {
        security = security.with_auth_protocol(to_snmp2_auth_protocol(auth_proto));
    }

    if let (Some(priv_proto), Some(priv_pass)) = (creds.priv_protocol, &creds.priv_password) {
        security = security.with_auth(snmp2::v3::Auth::AuthPriv {
            cipher: to_snmp2_cipher(priv_proto),
            privacy_password: priv_pass.as_bytes().to_vec(),
        });
    } else if creds.auth_password.is_some() {
        security = security.with_auth(snmp2::v3::Auth::AuthNoPriv);
    } else {
        security = security.with_auth(snmp2::v3::Auth::NoAuthNoPriv);
    }

    security
}

/// Convert our SnmpValue to snmp2::Value for SET operations.
fn to_snmp2_value<'a>(value: &'a SnmpValue) -> snmp2::Value<'a> {
    match value {
        SnmpValue::Integer(v) => snmp2::Value::Integer(*v),
        SnmpValue::OctetString(bytes) => snmp2::Value::OctetString(bytes),
        SnmpValue::ObjectIdentifier(oid) => {
            // Convert our OID to snmp2's OID for SET operations.
            // If conversion fails (shouldn't for valid OIDs), fall back to Null.
            match to_snmp2_oid(oid) {
                Ok(snmp_oid) => snmp2::Value::ObjectIdentifier(snmp_oid),
                Err(_) => snmp2::Value::Null,
            }
        }
        SnmpValue::IpAddress(addr) => snmp2::Value::IpAddress(*addr),
        SnmpValue::Counter32(v) => snmp2::Value::Counter32(*v),
        SnmpValue::Gauge32(v) => snmp2::Value::Unsigned32(*v),
        SnmpValue::TimeTicks(v) => snmp2::Value::Timeticks(*v),
        SnmpValue::Counter64(v) => snmp2::Value::Counter64(*v),
        SnmpValue::Opaque(bytes) => snmp2::Value::Opaque(bytes),
        SnmpValue::Null
        | SnmpValue::NoSuchObject
        | SnmpValue::NoSuchInstance
        | SnmpValue::EndOfMibView => snmp2::Value::Null,
    }
}
