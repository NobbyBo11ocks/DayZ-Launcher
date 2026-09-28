//! Typed errors for the core. Serialised to a plain string for IPC so the UI
//! never depends on Rust error internals. What reaches the screen is a sentence; the
//! technical detail goes to the log (row 16).

use serde::{Serialize, Serializer};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Windows could not open {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Why the file could not be parsed is in the log (`AppError::parse`).
    #[error("{} could not be read: it is not in the form the launcher expects.", .path.display())]
    Parse { path: PathBuf },
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn parse(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        let path = path.into();
        crate::log_warn!(
            "app",
            "could not parse {}: {}",
            path.display(),
            reason.into()
        );
        Self::Parse { path }
    }

    /// A failure the player cannot act on, said as "{what}. The details are on the Logs
    /// page." with the detail in the log: "cache lock poisoned" and "cache task failed: …"
    /// reached the screen as they were (row 16).
    pub fn logged(what: &str, detail: impl std::fmt::Display) -> Self {
        crate::log_error!("app", "{what}: {detail}");
        Self::Internal(format!("{what}. The details are on the Logs page."))
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
