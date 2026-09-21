//! Steam A2S server queries with DayZ's Server Browser Protocol v2 extension.
//! Spec and live evidence: docs/03-server-discovery-and-a2s.md (S-15, S-12, S-14, S-42).

pub mod client;
pub mod info;
pub mod packet;
pub mod players;
pub mod reader;
pub mod rules;
pub mod tags;

pub use client::{Client, Reply};
pub use info::Info;
pub use players::Players;
pub use rules::{DayzRules, Rules};
pub use tags::DayzTags;

#[derive(Debug, thiserror::Error)]
pub enum A2sError {
    #[error("packet truncated: needed {need} byte(s) at offset {at}")]
    Truncated { need: usize, at: usize },
    #[error("unexpected response type 0x{got:02X} (expected 0x{expected:02X})")]
    UnexpectedType { expected: u8, got: u8 },
    #[error("bad packet header 0x{0:08X}")]
    BadHeader(u32),
    #[error("bzip2-compressed split response is not supported")]
    Compressed,
    #[error("split fragment mismatch: {0}")]
    SplitMismatch(&'static str),
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("no answer within the timeout")]
    Timeout,
    #[error("challenge loop did not converge")]
    ChallengeLoop,
    #[error("unreachable (ICMP port unreachable)")]
    Unreachable,
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

pub type A2sResult<T> = Result<T, A2sError>;
