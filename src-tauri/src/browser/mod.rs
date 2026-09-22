//! Server browser data: row model, SQLite cache, population verification (docs/05 §2–3, docs/11).

pub mod cache;
pub mod dzsa;
pub mod model;
pub mod verify;

pub use cache::{
    Cache, HistoryEntry, ModCatalogEntry, ModsIndex, PopulationSample, ServerMods,
};
pub use model::ServerRow;
