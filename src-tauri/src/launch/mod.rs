//! Launching DayZ: `-mod=` junctions, argument line, `DayZ_BE.exe` spawn
//! (docs/02 §4–5, D-008…D-010).

pub mod args;
pub mod mods;
pub mod process;

pub use args::{build_args, display_command_line, LaunchSpec};
pub use mods::ensure_junctions;
pub use process::{spawn, Launched};
