use thiserror::Error;

/// Error type shared across crates. `status()` maps it to an HTTP status code.
///
/// The `Display` text reaches the user as is (HTTP `{"error": …}` bodies, UI banners, CLI), so
/// every variant carries a complete Italian sentence and adds no English prefix of its own:
/// the message of `NotFound` says what was not found ("sessione ses_1 non trovata"), that of
/// `Unsupported` why the device cannot do it, and so on.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Denied(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
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
    #[error("annullato")]
    Cancelled,
    #[error("errore di I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON non valido: {0}")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_has_no_english_prefix() {
        let cases = [
            (Error::NotFound("sessione ses_1 non trovata".into()), "sessione ses_1 non trovata", 404),
            (Error::Invalid("titolo vuoto".into()), "titolo vuoto", 400),
            (Error::Unsupported("serve il plugin nativo dell'app".into()), "serve il plugin nativo dell'app", 422),
            (Error::Denied("codice non valido".into()), "codice non valido", 403),
            (Error::Conflict("già in uso".into()), "già in uso", 409),
            (Error::Runtime("llama-server non risponde".into()), "llama-server non risponde", 500),
            (Error::Cancelled, "annullato", 499),
        ];
        for (e, text, status) in cases {
            assert_eq!(e.to_string(), text);
            assert_eq!(e.status(), status);
        }
        let io = Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "manca"));
        assert_eq!(io.to_string(), "errore di I/O: manca");
        let json = Error::from(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
        assert!(json.to_string().starts_with("JSON non valido: "), "{json}");
    }
}
