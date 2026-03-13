use mib_parser::Oid;
use tokio::sync::mpsc;

use crate::channel::{OperationType, SnmpRequest, SnmpResponse};
use crate::session::SnmpSession;

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
    pub fn spawn() -> (Self, mpsc::Receiver<SnmpResponse>) {
        let (request_tx, request_rx) = mpsc::channel::<SnmpRequest>(32);
        let (response_tx, response_rx) = mpsc::channel::<SnmpResponse>(64);

        tokio::task::spawn_blocking(move || {
            worker_loop(request_rx, response_tx);
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
) {
    let mut session: Option<SnmpSession> = None;

    while let Some(request) = request_rx.blocking_recv() {
        let response = match request {
            SnmpRequest::Connect(config) => {
                let dest = config.destination();
                match SnmpSession::new(config) {
                    Ok(new_session) => {
                        session = Some(new_session);
                        SnmpResponse::ok(
                            OperationType::Connect,
                            Oid::new(vec![]),
                            format!("Connected to {}", dest),
                        )
                    }
                    Err(e) => SnmpResponse::error(
                        OperationType::Connect,
                        Oid::new(vec![]),
                        format!("Connection failed: {}", e),
                    ),
                }
            }

            SnmpRequest::Disconnect => {
                session = None;
                SnmpResponse::ok(
                    OperationType::Disconnect,
                    Oid::new(vec![]),
                    "Disconnected".to_string(),
                )
            }

            SnmpRequest::Get(oid) => {
                if let Some(ref mut sess) = session {
                    match sess.get(&oid) {
                        Ok(value) => {
                            SnmpResponse::value(OperationType::Get, oid.clone(), oid, value)
                        }
                        Err(e) => SnmpResponse::error(OperationType::Get, oid, e.to_string()),
                    }
                } else {
                    SnmpResponse::error(OperationType::Get, oid, "No active session".to_string())
                }
            }

            SnmpRequest::GetNext(oid) => {
                if let Some(ref mut sess) = session {
                    match sess.get_next(&oid) {
                        Ok((next_oid, value)) => {
                            SnmpResponse::value(OperationType::GetNext, oid, next_oid, value)
                        }
                        Err(e) => SnmpResponse::error(OperationType::GetNext, oid, e.to_string()),
                    }
                } else {
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
                if let Some(ref mut sess) = session {
                    match sess.get_bulk(&oid, max_repetitions) {
                        Ok(values) => {
                            SnmpResponse::multi_value(OperationType::GetBulk, oid, values)
                        }
                        Err(e) => SnmpResponse::error(OperationType::GetBulk, oid, e.to_string()),
                    }
                } else {
                    SnmpResponse::error(
                        OperationType::GetBulk,
                        oid,
                        "No active session".to_string(),
                    )
                }
            }

            SnmpRequest::Walk(oid) => {
                if let Some(ref mut sess) = session {
                    match sess.walk(&oid) {
                        Ok(values) => SnmpResponse::multi_value(OperationType::Walk, oid, values),
                        Err(e) => SnmpResponse::error(OperationType::Walk, oid, e.to_string()),
                    }
                } else {
                    SnmpResponse::error(OperationType::Walk, oid, "No active session".to_string())
                }
            }

            SnmpRequest::Set { oid, value } => {
                if let Some(ref mut sess) = session {
                    match sess.set(&oid, &value) {
                        Ok(()) => {
                            SnmpResponse::ok(OperationType::Set, oid, "SET successful".to_string())
                        }
                        Err(e) => SnmpResponse::error(OperationType::Set, oid, e.to_string()),
                    }
                } else {
                    SnmpResponse::error(OperationType::Set, oid, "No active session".to_string())
                }
            }
        };

        if response_tx.blocking_send(response).is_err() {
            // Receiver dropped, TUI has shut down
            break;
        }
    }
}
