//! Steam and DayZ discovery on Windows. Evidence for every path and key:
//! docs/02-dayz-launch-mechanics.md (S-41, live machine) and docs/09 decisions.

pub mod cpp;
pub mod diagnostics;
pub mod locate;
pub mod official;
pub mod registry;
pub mod sdk;
pub mod vdf;
pub mod version;
pub mod workshop;

/// DayZ client App ID (docs/02 §1).
pub const DAYZ_APP_ID: u32 = 221_100;
