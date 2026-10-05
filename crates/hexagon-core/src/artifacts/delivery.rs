//! Fullstack QA 2026-10-05 #12: final acceptance rejected an API contract
//! legitimately revised in implementation because its original row was
//! superseded. Resolve an obligation's existing path/kind to its current version;
//! counting superseded rows or borrowing an unrelated kind would attest stale
//! or undelivered work. False negative costs another delivery/review; false
//! positive accepts unverified work, so absent anchors and rejected attempts fail.
use crate::db::Db;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Delivery {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub author: Option<String>,
    pub version: i64,
    pub stage: String,
}

#[derive(Serialize)]
pub(crate) struct Deliveries {
    pub items: Vec<Delivery>,
    pub missing: Vec<(String, String)>,
}

/// Registration identity only. Callers still verify materialization, readable
/// bodies and version-bound reviews; a registered row alone is not acceptance.
pub(crate) fn for_run(
    db: &Db,
    project: &str,
    run: &str,
    required: &[String],
) -> rusqlite::Result<Deliveries> {
    let required = serde_json::to_string(required)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let mut query = db.conn().prepare(
        "SELECT DISTINCT current.id,current.path,current.kind,current.author_agent_id,
            current.version,current.stage_run_id
         FROM artifacts anchor
         JOIN stage_runs original ON original.id=anchor.stage_run_id
            AND original.project_id=anchor.project_id
         JOIN artifacts current ON current.project_id=anchor.project_id
            AND current.path=anchor.path AND current.kind=anchor.kind
         JOIN stage_runs revision ON revision.id=current.stage_run_id
            AND revision.project_id=current.project_id
         WHERE anchor.project_id=?1 AND anchor.stage_run_id=?2
            AND anchor.kind IN (SELECT value FROM json_each(?3))
            AND anchor.status IN ('valid','stamped','superseded')
            AND current.status IN ('valid','stamped')
            AND revision.seq>=original.seq
            AND revision.state IN ('active','waiting_stamp','done')
            AND revision.rowid=(SELECT MAX(rowid) FROM stage_runs latest
                WHERE latest.project_id=revision.project_id AND latest.seq=revision.seq)
            AND NOT EXISTS(SELECT 1 FROM artifacts newer
                WHERE newer.project_id=current.project_id AND newer.path=current.path
                    AND newer.version>current.version)
         ORDER BY current.id",
    )?;
    let deliveries = query
        .query_map(rusqlite::params![project, run, required], |r| {
            Ok(Delivery {
                id: r.get(0)?,
                path: r.get(1)?,
                kind: r.get(2)?,
                author: r.get(3)?,
                version: r.get(4)?,
                stage: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // Keep unresolved obligations: another file of the same kind cannot hide
    // a retyped/deleted/rejected version of an originally delivered path.
    let mut anchors = db.conn().prepare("SELECT DISTINCT path,kind FROM artifacts WHERE project_id=?1 AND stage_run_id=?2 AND status IN ('valid','stamped','superseded') AND kind IN (SELECT value FROM json_each(?3)) ORDER BY path,kind")?;
    let missing = anchors
        .query_map(rusqlite::params![project, run, required], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(path, kind)| {
            !deliveries
                .iter()
                .any(|d| &d.path == path && &d.kind == kind)
        })
        .collect::<Vec<_>>();
    // QA 2026-10-05: resolving registered versions is a read projection.
    // Repeated evidence polls previously generated duplicate refusals here;
    // actual advance/stamp attempts now report the decision boundary.
    Ok(Deliveries {
        items: deliveries,
        missing,
    })
}
