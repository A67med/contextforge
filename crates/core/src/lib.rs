//! ContextForge core: a local-first, provenance-aware memory engine shared by AI coding agents.
//!
//! The [`Store`] is the entry point. Every process (desktop app, CLI, MCP server, lifecycle hook) opens its own
//! `Store`; SQLite in WAL mode with immediate write transactions makes concurrent use safe.

pub mod backup;
pub mod capture;
pub mod context;
pub mod db;
pub mod error;
pub mod freshness;
pub mod git;
pub mod handoff;
pub mod memories;
pub mod model;
pub mod paths;
pub mod privacy;
pub mod process;
pub mod projects;
pub mod search;
pub mod sessions;
pub mod settings;
pub mod stats;
pub mod util;

pub use db::{OpenOptions, Store, data_dir};
pub use error::{Error, Result};
pub use settings::Settings;

/// Version of this build, shared by the CLI, MCP server and desktop app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
