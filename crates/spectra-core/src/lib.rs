//! OpenSpectra core: change discovery, capability spec discovery, and drift
//! detection, reverse-engineered from the closed-source `spectra` CLI (v2.3.1).

pub mod anchors;
pub mod archive;
pub mod calibration;
pub mod change;
pub mod config;
pub mod drift;
pub mod git;
pub mod init;
mod names;
pub mod spec;
pub mod tasks;
pub mod touched;

pub use change::{Change, ChangeMetadata};
pub use config::Config;
pub use drift::{analyze, DriftReport};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
