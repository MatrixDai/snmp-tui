use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use mib_parser::Oid;
use tokio::sync::mpsc;

use crate::channel::{OperationType, SnmpRequest, SnmpResponse};
use crate::session::SnmpSession;

/// Simple file-based debug logger for SNMP operations.
struct DebugLog {
    file: Mutex<std::fs::File>,
}

impl DebugLog {
    fn new(path: &PathBuf) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    fn log(&self, msg: &str) {
        if let Ok(mut f) = self.file.lock() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let secs = now.as_secs();
            let hours = (secs / 3600) % 24;
            let minutes = (secs / 60) % 60;
            let seconds = secs % 60;
            let _ = writeln!(f, "[{:02}:{:02}:{:02}] {}", hours, minutes, seconds, msg);
            let _ = f.flush();
        }
    }
}

/// Background SNMP worker that processes requests on a dedicated thread.
///
/// The worker runs on a tokio blocking thread (via `spawn_blocking`) and
/// communicates with the TUI event loop via mpsc channels.
pub struct SnmpWorker {
    request_tx: mpsc::Sender<SnmpRequest>,
}

impl SnmpWorker {
    /// Spawn the background worker. Returns the worker handle and a receiver
    /// for SNMP responses.
    ///
    /// The worker processes requests sequentially. It maintains an optional
    /// `SnmpSession` that is created on `Connect` and destroyed on `Disconnect`.
    ///
    /// If `debug_log` is provided, debug messages are written to that file path.
    pub fn spawn(debug_log: Option<PathBuf>) -> (Self, mpsc::Receiver<SnmpResponse>) {
        let (request_tx, request_rx) = mpsc::channel::<SnmpRequest>(32);
        let (response_tx, response_rx) = mpsc::channel::<SnmpResponse>(64);

        tokio::task::spawn_blocking(move || {
            let logger = debug_log.and_then(|path| {
                DebugLog::new(&path)
                    .map_err(|e| eprintln!("Warning: Failed to open debug log: {}", e))
                    .ok()
            });
            worker_loop(request_rx, response_tx, logger.as_ref());
        });

        (Self { request_tx }, response_rx)
    }

    /// Send a request to the background worker.
    pub async fn send(
        &self,
        request: SnmpRequest,
    ) -> Result<(), mpsc::error::SendError<SnmpRequest>> {
        self.request_tx.send(request).await
    }

    /// Try to send a request without waiting (non-blocking).
    #[allow(clippy::result_large_err)]
    pub fn try_send(
        &self,
        request: SnmpRequest,
    ) -> Result<(), mpsc::error::TrySendError<SnmpRequest>> {
        self.request_tx.try_send(request)
    }
}

/// The main worker loop that processes SNMP requests.
fn worker_loop(
    mut request_rx: mpsc::Receiver<SnmpRequest>,
    response_tx: mpsc::Sender<SnmpResponse>,
    debug: Option<&DebugLog>,
) {
    let mut session: Option<SnmpSession> = None;

    macro_rules! dbg_log {
        ($($arg:tt)*) => {
            if let Some(log) = debug {
                log.log(&format!($($arg)*));
            }
        };
    }

    dbg_log!("Worker started, waiting for requests");

    while let Some(request) = request_rx.blocking_recv() {
        let response = match request {
            SnmpRequest::Connect(config) => {
                let dest = config.destination();
                dbg_log!(
                    "CONNECT to {} version={} community={} timeout={}ms retries={}",
                    dest,
                    config.version,
                    config.community,
                    config.timeout_ms,
                    config.retries
                );
                if let Some(ref creds) = config.v3_credentials {
                    dbg_log!(
                        "  v3: user={} auth={:?} priv={:?}",
                        creds.username,
                        creds.auth_protocol,
                        creds.priv_protocol
                    );
                }
                match SnmpSession::new(config) {
                    Ok(new_session) => {
                        dbg_log!("  Session created successfully (note: UDP, no actual handshake)");
                        session = Some(new_session);
                        SnmpResponse::ok(
                            OperationType::Connect,
                            Oid::new(vec![]),
                            format!("Connected to {}", dest),
                        )
                    }
                    Err(e) => {
                        dbg_log!("  Session creation FAILED: {:?}", e);
                        SnmpResponse::error(
                            OperationType::Connect,
                            Oid::new(vec![]),
                            format!("Connection failed: {}", e),
                        )
                    }
                }
            }

            SnmpRequest::Disconnect => {
                dbg_log!("DISCONNECT");
                session = None;
                SnmpResponse::ok(
                    OperationType::Disconnect,
                    Oid::new(vec![]),
                    "Disconnected".to_string(),
                )
            }

            SnmpRequest::Get(oid) => {
                dbg_log!("GET {}", oid);
                if let Some(ref mut sess) = session {
                    dbg_log!("  Session config: {:?}", sess.config());
                    match sess.get(&oid) {
                        Ok(value) => {
                            dbg_log!("  Result: {}", value);
                            SnmpResponse::value(OperationType::Get, oid.clone(), oid, value)
                        }
                        Err(e) => {
                            dbg_log!("  ERROR: {:?}", e);
                            SnmpResponse::error(OperationType::Get, oid, e.to_string())
                        }
                    }
                } else {
                    dbg_log!("  ERROR: No active session");
                    SnmpResponse::error(OperationType::Get, oid, "No active session".to_string())
                }
            }

            SnmpRequest::GetNext(oid) => {
                dbg_log!("GETNEXT {}", oid);
                if let Some(ref mut sess) = session {
                    match sess.get_next(&oid) {
                        Ok((next_oid, value)) => {
                            dbg_log!("  Result: {} = {}", next_oid, value);
                            SnmpResponse::value(OperationType::GetNext, oid, next_oid, value)
                        }
                        Err(e) => {
                            dbg_log!("  ERROR: {:?}", e);
                            SnmpResponse::error(OperationType::GetNext, oid, e.to_string())
                        }
                    }
                } else {
                    dbg_log!("  ERROR: No active session");
                    SnmpResponse::error(
                        OperationType::GetNext,
                        oid,
                        "No active session".to_string(),
                    )
                }
            }

            SnmpRequest::GetBulk {
                oid,
                max_repetitions,
            } => {
                dbg_log!("GETBULK {} max_rep={}", oid, max_repetitions);
                if let Some(ref mut sess) = session {
                    match sess.get_bulk(&oid, max_repetitions) {
                        Ok(values) => {
                            dbg_log!("  Result: {} entries", values.len());
                            SnmpResponse::multi_value(OperationType::GetBulk, oid, values)
                        }
                        Err(e) => {
                            dbg_log!("  ERROR: {:?}", e);
                            SnmpResponse::error(OperationType::GetBulk, oid, e.to_string())
                        }
                    }
                } else {
                    dbg_log!("  ERROR: No active session");
                    SnmpResponse::error(
                        OperationType::GetBulk,
                        oid,
                        "No active session".to_string(),
                    )
                }
            }

            SnmpRequest::Walk(oid) => {
                dbg_log!("WALK {}", oid);
                if let Some(ref mut sess) = session {
                    match sess.walk(&oid) {
                        Ok(values) => {
                            dbg_log!("  Result: {} entries", values.len());
                            SnmpResponse::multi_value(OperationType::Walk, oid, values)
                        }
                        Err(e) => {
                            dbg_log!("  ERROR: {:?}", e);
                            SnmpResponse::error(OperationType::Walk, oid, e.to_string())
                        }
                    }
                } else {
                    dbg_log!("  ERROR: No active session");
                    SnmpResponse::error(OperationType::Walk, oid, "No active session".to_string())
                }
            }

            SnmpRequest::Set { oid, value } => {
                dbg_log!("SET {} = {:?}", oid, value);
                if let Some(ref mut sess) = session {
                    match sess.set(&oid, &value) {
                        Ok(()) => {
                            dbg_log!("  SET successful");
                            SnmpResponse::ok(OperationType::Set, oid, "SET successful".to_string())
                        }
                        Err(e) => {
                            dbg_log!("  ERROR: {:?}", e);
                            SnmpResponse::error(OperationType::Set, oid, e.to_string())
                        }
                    }
                } else {
                    dbg_log!("  ERROR: No active session");
                    SnmpResponse::error(OperationType::Set, oid, "No active session".to_string())
                }
            }
        };

        if response_tx.blocking_send(response).is_err() {
            dbg_log!("Response receiver dropped, shutting down worker");
            break;
        }
    }

    dbg_log!("Worker loop ended");
}
