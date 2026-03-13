use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Grammar(String),

    #[error("unresolved import: {module}::{name}")]
    UnresolvedImport { module: String, name: String },
}
