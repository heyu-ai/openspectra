//! OpenSpectra core: change discovery, capability spec discovery, and drift
//! detection, reverse-engineered from the closed-source `spectra` CLI (v2.3.1).

pub mod analyze;
pub mod anchors;
pub mod archive;
pub mod artifact;
pub mod calibration;
pub mod change;
pub mod config;
pub mod drift;
mod fsutil;
pub mod git;
pub mod init;
pub mod instructions;
mod names;
pub mod schema;
pub mod spec;
pub mod tasks;
#[cfg(test)]
pub(crate) mod test_support;
pub mod touched;
pub mod update;
pub(crate) mod update_manifest;
pub mod validate;

pub use change::{Change, ChangeMetadata};
pub use config::Config;
pub use drift::{analyze, DriftReport};
pub use validate::{validate_all_active, ValidateReport};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
