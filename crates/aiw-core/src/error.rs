use thiserror::Error;

/// Error type shared across crates. `status()` maps it to an HTTP status code.
#[derive(Debug, Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not supported on this device: {0}")]
    Unsupported(String),
    #[error("denied by policy: {0}")]
    Denied(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    /// A provider rejected the credentials (HTTP 401): a token is missing or invalid.
    #[error("{0}")]
    Unauthorized(String),
    /// Not enough free disk space (HTTP 507).
    #[error("{0}")]
    InsufficientStorage(String),
    /// A remote service failed or is unreachable (HTTP 502).
    #[error("{0}")]
    Upstream(String),
    #[error("cancelled")]
    Cancelled,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn status(&self) -> u16 {
        match self {
            Error::NotFound(_) => 404,
            Error::Invalid(_) | Error::Json(_) => 400,
            Error::Unsupported(_) => 422,
            Error::Unauthorized(_) => 401,
            Error::Denied(_) => 403,
            Error::Conflict(_) => 409,
            Error::Cancelled => 499,
            Error::InsufficientStorage(_) => 507,
            Error::Upstream(_) => 502,
            _ => 500,
        }
    }
    pub fn other(msg: impl std::fmt::Display) -> Self {
        Error::Other(msg.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
