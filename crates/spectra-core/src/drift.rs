//! Drift detection: assembles the four dimensions into a [`DriftReport`] whose
//! JSON serialization matches the reference `spectra drift --json` schema
//! field-for-field.

use anyhow::Result;
use chrono::{Local, NaiveDate};
use serde::Serialize;

use crate::anchors::{self, BrokenAnchor, Resolver};
use crate::calibration;
use crate::change::Change;
use crate::config::Config;
use crate::git;
use crate::tasks::{self, TaskCollision};

#[derive(Debug, Clone, Copy, Serialize)]
pub enum DimensionKind {
    Time,
    Structure,
    Tasks,
    Environment,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dimension {
    pub kind: DimensionKind,
    pub status: String,
    pub score: i64,
    pub contributes_to_total: bool,
}

/// Field order here is intentional: it matches the reference JSON byte-for-byte.
#[derive(Debug, Clone, Serialize)]
pub struct DriftReport {
    pub change_id: String,
    pub created: Option<String>,
    /// Always `null` in observed reference output (v2.3.1); semantics of any
    /// non-null value are undetermined. TODO(reverse-engineering): confirm.
    pub last_commit: Option<String>,
    pub dimensions: Vec<Dimension>,
    pub broken_anchors: Vec<BrokenAnchor>,
    pub tasks_maybe_resolved: Vec<TaskCollision>,
    pub tasks_blocked_external: Vec<TaskCollision>,
    pub commits_since_created: u64,
    pub total_score: i64,
    pub severity: String,
    pub primary_recommendation: String,
}

/// Days between `created` (YYYY-MM-DD) and today, or an error word.
fn time_dimension(created: Option<&str>) -> Dimension {
    let (status, score) = match created {
        None => ("no created date".to_string(), 0),
        Some(raw) => match NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
            Err(_) => ("invalid created date".to_string(), 0),
            Ok(date) => {
                let days = (Local::now().date_naive() - date).num_days();
                let (word, score) = calibration::time_bucket(days);
                (format!("{word} ({days}d)"), score)
            }
        },
    };
    Dimension {
        kind: DimensionKind::Time,
        status,
        score,
        contributes_to_total: true,
    }
}

/// Run drift for an already-loaded change.
pub fn analyze(cfg: &Config, change: &Change) -> Result<DriftReport> {
    let created = change.metadata.created.as_deref();

    // --- Time ---------------------------------------------------------------
    let time = time_dimension(created);

    // --- Structure (broken anchors) -----------------------------------------
    let design_path = change.design_md();
    let (structure, broken_anchors, decay) = if design_path.exists() {
        let design = std::fs::read_to_string(&design_path)?;
        let anchors = anchors::extract(&design);
        let total = anchors.len();
        let tracked = git::ls_files(&cfg.root);
        let resolver = Resolver {
            root: &cfg.root,
            tracked: &tracked,
        };
        let broken = resolver.broken(&anchors);
        let decay = if total == 0 {
            0.0
        } else {
            broken.len() as f64 / total as f64
        };
        let status = format!("{}/{} anchors broken", broken.len(), total);
        // Structure score is category-weighted: broken CliFlags raise the score,
        // broken FilePaths only raise the decay band. See calibration::structure_score.
        let broken_cliflags = broken.iter().filter(|b| b.category == "CliFlag").count();
        let score = calibration::structure_score(broken.len(), broken_cliflags, total);
        (
            Dimension {
                kind: DimensionKind::Structure,
                status,
                score,
                contributes_to_total: true,
            },
            broken,
            decay,
        )
    } else {
        (
            Dimension {
                kind: DimensionKind::Structure,
                status: "design absent".to_string(),
                score: 0,
                contributes_to_total: true,
            },
            Vec::new(),
            0.0,
        )
    };

    // --- Tasks --------------------------------------------------------------
    let tasks_path = change.tasks_md();
    let task_list = if tasks_path.exists() {
        tasks::parse(&std::fs::read_to_string(&tasks_path)?)
    } else {
        Vec::new()
    };
    let analysis = tasks::analyze(
        &cfg.root,
        &change.name,
        &task_list,
        change.started_sha.as_deref(),
        created,
    );
    let blocked = analysis.blocked_external.len();
    let maybe = analysis.maybe_resolved.len();
    let tasks_dim = Dimension {
        kind: DimensionKind::Tasks,
        status: format!("{blocked} blocked, {maybe} maybe-done"),
        score: calibration::tasks_score(blocked, maybe),
        contributes_to_total: true,
    };

    // --- Environment (display only) -----------------------------------------
    let commits_since_created = match created {
        Some(date) => git::commits_since(&cfg.root, date),
        None => 0,
    };
    let env = Dimension {
        kind: DimensionKind::Environment,
        status: format!("{commits_since_created} commits"),
        score: 0,
        contributes_to_total: false,
    };

    // --- Aggregate ----------------------------------------------------------
    let dimensions = vec![time, structure, tasks_dim, env];
    let total_score: i64 = dimensions
        .iter()
        .filter(|d| d.contributes_to_total)
        .map(|d| d.score)
        .sum();
    let severity = calibration::severity(total_score, decay).to_string();
    let primary_recommendation = calibration::primary_recommendation(&severity, &change.name);

    Ok(DriftReport {
        change_id: change.name.clone(),
        created: created.map(str::to_string),
        last_commit: None,
        dimensions,
        broken_anchors,
        tasks_maybe_resolved: analysis.maybe_resolved,
        tasks_blocked_external: analysis.blocked_external,
        commits_since_created,
        total_score,
        severity,
        primary_recommendation,
    })
}

impl DriftReport {
    /// CI exit code: 0 for light, non-zero otherwise (gate on medium/heavy).
    pub fn exit_code(&self) -> i32 {
        match self.severity.as_str() {
            "light" => 0,
            "medium" => 1,
            _ => 2,
        }
    }
}
