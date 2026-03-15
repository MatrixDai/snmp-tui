use mib_parser::Oid;
use snmp_client::{
    AuthProtocol, OperationType, PrivProtocol, SnmpConfig, SnmpError, SnmpRequest, SnmpResponse,
    SnmpResult, SnmpValue, SnmpVersion, SnmpWorker, V3Credentials,
};

// ============================================================
// Config tests
// ============================================================

#[test]
fn config_default_values() {
    let config = SnmpConfig::default();
    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 161);
    assert_eq!(config.version, SnmpVersion::V2c);
    assert_eq!(config.read_community, "public");
    assert_eq!(config.write_community, "private");
    assert_eq!(config.timeout_ms, 5000);
    assert_eq!(config.retries, 1);
    assert!(config.v3_credentials.is_none());
}

#[test]
fn config_destination_format() {
    let config = SnmpConfig {
        host: "192.168.1.1".to_string(),
        port: 1161,
        ..Default::default()
    };
    assert_eq!(config.destination(), "192.168.1.1:1161");
}

#[test]
fn snmp_version_display() {
    assert_eq!(SnmpVersion::V1.to_string(), "v1");
    assert_eq!(SnmpVersion::V2c.to_string(), "v2c");
    assert_eq!(SnmpVersion::V3.to_string(), "v3");
}

// ============================================================
// SnmpValue conversion and display tests
// ============================================================

#[test]
fn value_display_all_variants() {
    assert_eq!(SnmpValue::Integer(42).to_string(), "42");
    assert_eq!(SnmpValue::Counter32(100).to_string(), "100");
    assert_eq!(SnmpValue::Gauge32(50).to_string(), "50");
    assert_eq!(SnmpValue::Counter64(999999).to_string(), "999999");
    assert_eq!(SnmpValue::Null.to_string(), "NULL");
    assert_eq!(SnmpValue::NoSuchObject.to_string(), "noSuchObject");
    assert_eq!(SnmpValue::NoSuchInstance.to_string(), "noSuchInstance");
    assert_eq!(SnmpValue::EndOfMibView.to_string(), "endOfMibView");
}

#[test]
fn value_equality() {
    assert_eq!(SnmpValue::Integer(42), SnmpValue::Integer(42));
    assert_ne!(SnmpValue::Integer(42), SnmpValue::Integer(43));
    assert_eq!(
        SnmpValue::OctetString(b"test".to_vec()),
        SnmpValue::OctetString(b"test".to_vec())
    );
    assert_eq!(
        SnmpValue::IpAddress([10, 0, 0, 1]),
        SnmpValue::IpAddress([10, 0, 0, 1])
    );
}

// ============================================================
// Error tests
// ============================================================

#[test]
fn error_display() {
    let e = SnmpError::Timeout(5000);
    assert_eq!(e.to_string(), "timeout after 5000ms");

    let e = SnmpError::NoSuchObject;
    assert_eq!(e.to_string(), "no such object");

    let e = SnmpError::ErrorStatus {
        code: 2,
        message: "noSuchName".to_string(),
    };
    assert!(e.to_string().contains("noSuchName"));
}

// ============================================================
// Channel types tests
// ============================================================

#[test]
fn operation_type_display() {
    assert_eq!(OperationType::Get.to_string(), "GET");
    assert_eq!(OperationType::GetNext.to_string(), "GETNEXT");
    assert_eq!(OperationType::GetBulk.to_string(), "GETBULK");
    assert_eq!(OperationType::Walk.to_string(), "WALK");
    assert_eq!(OperationType::Set.to_string(), "SET");
    assert_eq!(OperationType::Connect.to_string(), "CONNECT");
    assert_eq!(OperationType::Disconnect.to_string(), "DISCONNECT");
}

#[test]
fn snmp_response_constructors() {
    let oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 1]);

    let resp = SnmpResponse::ok(OperationType::Connect, oid.clone(), "connected".to_string());
    assert_eq!(resp.operation, OperationType::Connect);
    assert!(matches!(resp.result, SnmpResult::Ok(_)));

    let resp = SnmpResponse::error(OperationType::Get, oid.clone(), "timeout".to_string());
    assert_eq!(resp.operation, OperationType::Get);
    assert!(matches!(resp.result, SnmpResult::Error(_)));

    let resp = SnmpResponse::value(
        OperationType::Get,
        oid.clone(),
        oid.clone(),
        SnmpValue::Integer(42),
    );
    assert!(matches!(resp.result, SnmpResult::Value(_, _)));

    let resp = SnmpResponse::multi_value(
        OperationType::Walk,
        oid.clone(),
        vec![(oid.clone(), SnmpValue::Integer(1))],
    );
    assert!(matches!(resp.result, SnmpResult::MultiValue(_)));
}

#[test]
fn snmp_request_variants() {
    let oid = Oid::new(vec![1, 3, 6, 1, 2, 1]);

    // Just verify we can construct all variants
    let _get = SnmpRequest::Get(oid.clone());
    let _next = SnmpRequest::GetNext(oid.clone());
    let _bulk = SnmpRequest::GetBulk {
        oid: oid.clone(),
        max_repetitions: 10,
    };
    let _walk = SnmpRequest::Walk(oid.clone());
    let _set = SnmpRequest::Set {
        oid: oid.clone(),
        value: SnmpValue::Integer(42),
    };
    let _connect = SnmpRequest::Connect(SnmpConfig::default());
    let _disconnect = SnmpRequest::Disconnect;
}

// ============================================================
// V3 credentials tests
// ============================================================

#[test]
fn v3_config_auth_no_priv() {
    let config = SnmpConfig {
        version: SnmpVersion::V3,
        v3_credentials: Some(V3Credentials {
            username: "testuser".to_string(),
            auth_protocol: Some(AuthProtocol::Sha1),
            auth_password: Some("authpass123".to_string()),
            priv_protocol: None,
            priv_password: None,
        }),
        ..Default::default()
    };
    assert_eq!(config.version, SnmpVersion::V3);
    let creds = config.v3_credentials.unwrap();
    assert_eq!(creds.username, "testuser");
    assert_eq!(creds.auth_protocol, Some(AuthProtocol::Sha1));
    assert!(creds.priv_protocol.is_none());
}

#[test]
fn v3_config_auth_priv() {
    let config = SnmpConfig {
        version: SnmpVersion::V3,
        v3_credentials: Some(V3Credentials {
            username: "admin".to_string(),
            auth_protocol: Some(AuthProtocol::Sha256),
            auth_password: Some("authpass".to_string()),
            priv_protocol: Some(PrivProtocol::Aes128),
            priv_password: Some("privpass".to_string()),
        }),
        ..Default::default()
    };
    let creds = config.v3_credentials.unwrap();
    assert_eq!(creds.priv_protocol, Some(PrivProtocol::Aes128));
    assert_eq!(creds.priv_password.as_deref(), Some("privpass"));
}

// ============================================================
// Session tests (connection failure expected — no agent running)
// ============================================================

#[test]
fn session_connect_to_nonexistent_host_timeout() {
    // Run on a thread with a larger stack to avoid stack overflow
    // in CI environments where the default stack is small and
    // snmp2's internal allocations are deep.
    let result = std::thread::Builder::new()
        .stack_size(4 * 1024 * 1024) // 4 MB
        .spawn(|| {
            use snmp_client::SnmpSession;

            // Use a very short timeout to a non-routable address
            let config = SnmpConfig {
                host: "192.0.2.1".to_string(), // TEST-NET, should not respond
                port: 16199,
                timeout_ms: 100,
                retries: 0,
                ..Default::default()
            };

            // Session creation should succeed (UDP — just creates a socket)
            let mut session =
                SnmpSession::new(config).expect("UDP session creation should succeed");

            // GET should fail (timeout or connection refused)
            let oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 1, 0]);
            let result = session.get(&oid);
            assert!(result.is_err(), "GET to non-existent host should fail");
        })
        .expect("failed to spawn test thread")
        .join();

    result.expect("test thread panicked");
}

#[test]
fn session_v3_without_credentials() {
    use snmp_client::SnmpSession;

    let config = SnmpConfig {
        version: SnmpVersion::V3,
        v3_credentials: None,
        ..Default::default()
    };

    let result = SnmpSession::new(config);
    assert!(result.is_err(), "V3 without credentials should fail");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("credentials"),
        "Error should mention credentials: {}",
        err
    );
}

// ============================================================
// Worker tests (no live agent, tests request/response flow)
// ============================================================

#[tokio::test]
async fn worker_disconnect_without_connect() {
    let (worker, mut response_rx) = SnmpWorker::spawn(None);

    worker
        .send(SnmpRequest::Disconnect)
        .await
        .expect("send should succeed");

    let resp = response_rx.recv().await.expect("should receive response");
    assert_eq!(resp.operation, OperationType::Disconnect);
    assert!(matches!(resp.result, SnmpResult::Ok(_)));
}

#[tokio::test]
async fn worker_get_without_connect() {
    let (worker, mut response_rx) = SnmpWorker::spawn(None);

    let oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 1, 0]);
    worker
        .send(SnmpRequest::Get(oid))
        .await
        .expect("send should succeed");

    let resp = response_rx.recv().await.expect("should receive response");
    assert_eq!(resp.operation, OperationType::Get);
    assert!(
        matches!(resp.result, SnmpResult::Error(ref msg) if msg.contains("No active session")),
        "Should report no active session, got: {:?}",
        resp.result
    );
}

#[tokio::test]
async fn worker_walk_without_connect() {
    let (worker, mut response_rx) = SnmpWorker::spawn(None);

    let oid = Oid::new(vec![1, 3, 6, 1, 2, 1]);
    worker
        .send(SnmpRequest::Walk(oid))
        .await
        .expect("send should succeed");

    let resp = response_rx.recv().await.expect("should receive response");
    assert_eq!(resp.operation, OperationType::Walk);
    assert!(matches!(resp.result, SnmpResult::Error(_)));
}

#[tokio::test]
async fn worker_set_without_connect() {
    let (worker, mut response_rx) = SnmpWorker::spawn(None);

    let oid = Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 5, 0]);
    worker
        .send(SnmpRequest::Set {
            oid,
            value: SnmpValue::OctetString(b"test".to_vec()),
        })
        .await
        .expect("send should succeed");

    let resp = response_rx.recv().await.expect("should receive response");
    assert_eq!(resp.operation, OperationType::Set);
    assert!(matches!(resp.result, SnmpResult::Error(_)));
}

#[tokio::test]
async fn worker_connect_to_unreachable_host() {
    let (worker, mut response_rx) = SnmpWorker::spawn(None);

    let config = SnmpConfig {
        host: "192.0.2.1".to_string(),
        port: 16199,
        timeout_ms: 100,
        retries: 0,
        ..Default::default()
    };

    worker
        .send(SnmpRequest::Connect(config))
        .await
        .expect("send should succeed");

    let resp = response_rx.recv().await.expect("should receive response");
    assert_eq!(resp.operation, OperationType::Connect);
    // UDP connect should succeed (no handshake), so we get Ok
    assert!(matches!(resp.result, SnmpResult::Ok(_)));
}
