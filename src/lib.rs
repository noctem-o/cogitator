pub mod agent;
pub mod canonical_json;
pub mod chaos;
pub mod drift;
pub mod eval;
pub mod gauntlet; // legacy compatibility re-exports (deprecated name)
pub mod hex; // NEW: Centralized hex encoding utilities
pub mod io_utils;
pub mod llm;
pub mod model;
pub mod nix_provenance;
pub mod ordeal;
pub mod report;
pub mod tooling;
pub mod trace;
pub mod verify;
pub mod witness;

#[cfg(feature = "tui")]
pub mod tui;
