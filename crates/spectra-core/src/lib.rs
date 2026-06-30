//! OpenSpectra core: change discovery and drift detection, reverse-engineered
//! from the closed-source `spectra` CLI (v2.3.1).

pub mod anchors;
pub mod calibration;
pub mod change;
pub mod config;
pub mod drift;
pub mod git;
pub mod tasks;

pub use change::{Change, ChangeMetadata};
pub use config::Config;
pub use drift::{DriftReport, analyze};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
