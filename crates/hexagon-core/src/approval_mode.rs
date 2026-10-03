//! Project access approval, distinct from coordination autonomy.
//! Owner decision: agent-autonomy-grill-2026-10-01, Q3/Q6/Q10/Q11/Q15.
use crate::db::Db;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS,
)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ApprovalMode {
    #[default]
    Restricted,
    Assisted,
    Broad,
}

impl ApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restricted => "restricted",
            Self::Assisted => "assisted",
            Self::Broad => "broad",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ApprovalModeStatus {
    pub project_id: String,
    /// Canonical shell workspace identity; core-only reads have no host binding.
    pub project_root: Option<String>,
    pub mode: ApprovalMode,
}

pub fn read(db: &Db, project_id: &str) -> Result<ApprovalModeStatus, rusqlite::Error> {
    let raw: String = db.conn().query_row(
        "SELECT approval_mode FROM projects WHERE id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    // Unknown/corrupt persisted modes never enlarge a grant. A false negative
    // costs one owner question; a false positive costs an unreviewed side effect.
    let mode = match raw.as_str() {
        "assisted" => ApprovalMode::Assisted,
        "broad" => ApprovalMode::Broad,
        _ => ApprovalMode::Restricted,
    };
    Ok(ApprovalModeStatus {
        project_id: project_id.into(),
        project_root: None,
        mode,
    })
}

pub fn set(
    db: &Db,
    project_id: &str,
    mode: ApprovalMode,
) -> Result<ApprovalModeStatus, rusqlite::Error> {
    let started = std::time::Instant::now();
    db.conn().execute(
        "UPDATE projects SET approval_mode=?1 WHERE id=?2",
        rusqlite::params![mode.as_str(), project_id],
    )?;
    let status = read(db, project_id)?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(project_id),
        None,
        None,
        None,
        "approval_mode",
        mode.as_str(),
        started,
    );
    Ok(status)
}
