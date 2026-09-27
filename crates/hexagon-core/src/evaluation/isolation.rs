//! D05: public task projections, isolated generation roots, durable disclosure.
use super::{config, err, EvaluationTask};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationContext {
    pub id: String,
    pub batch_id: String,
    pub workspace: String,
    pub task_ids: Vec<String>,
    pub material_fingerprint: String,
    pub tainted: bool,
    pub eligible_for_generation: bool,
}

/// Explicit allow-list: adding fields to the host manifest cannot expose an
/// oracle to the generator. Neither development nor heldout answers are copied.
#[derive(Serialize)]
struct PublicTask<'a> {
    id: &'a str,
    requirements: &'a str,
    source: &'a str,
    license: &'a str,
    revision: &'a str,
    dependencies: &'a [String],
    allowed_paths: &'a [String],
    files: &'a BTreeMap<String, String>,
    initial_changes: &'a BTreeMap<String, String>,
    untracked_files: &'a BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationReport {
    pub id: String,
    pub runtime_fingerprint: String,
    pub passed: bool,
    pub public_requirements_readable: bool,
    pub filesystem_denied: bool,
    pub terminal_denied: bool,
    pub child_process_denied: bool,
    pub index_clean: bool,
    pub messages_clean: bool,
    pub host_material_unchanged: bool,
    pub evidence_kind: String,
}

pub(crate) fn task_identity(task: &EvaluationTask) -> io::Result<String> {
    let mut value = serde_json::to_value(task)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| err("task is not an object"))?;
    // D05: names, distribution metadata and claimed revisions are not novelty.
    // False rejection needs a fresh task; false acceptance launders heldout use.
    // Ticket 08 review: reordering dependency declarations laundered an
    // exposed task into a new identity. Environment/scope labels remain fully
    // frozen by config.rs, but do not establish a fresh heldout problem.
    for key in [
        "id",
        "source",
        "license",
        "revision",
        "category",
        "dependencies",
        "allowed_paths",
        "safety", // D06: changing claimed safety scope cannot freshen a disclosed task.
    ] {
        object.remove(key);
    }
    if let Some(fragments) = object
        .get_mut("preserved_fragments")
        .and_then(serde_json::Value::as_object_mut)
    {
        for parts in fragments.values_mut() {
            if let Some(parts) = parts.as_array_mut() {
                parts.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                parts.dedup();
            }
        }
    }
    config::digest(&value)
}

pub(crate) fn retired(db: &Db, task: &EvaluationTask) -> io::Result<bool> {
    db.conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_disclosures WHERE task_key=?1)",
            [task_identity(task)?],
            |r| r.get(0),
        )
        .map_err(err)
}

pub(crate) fn prepare(db: &Db, root: &Path, batch_id: &str) -> io::Result<GenerationContext> {
    let batch = config::read(db, batch_id)?;
    let id = format!(
        "generation-{}",
        db.next_id("evaluation_generation").map_err(err)?
    );
    let parent = root.join(".hexagon/evaluation-generations");
    std::fs::create_dir_all(&parent)?;
    let workspace = parent.join(&id);
    std::fs::create_dir(&workspace)?;
    std::fs::create_dir(workspace.join(".hexagon"))?;
    std::fs::write(
        workspace.join(".hexagon/evaluation-worker"),
        "private-state-v1",
    )?;
    let tasks: Vec<_> = batch
        .request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .filter(|c| c.split == "development")
        .map(|c| {
            let t = &c.task;
            PublicTask {
                id: &t.id,
                requirements: &t.requirements,
                source: &t.source,
                license: &t.license,
                revision: &t.revision,
                dependencies: &t.dependencies,
                allowed_paths: &t.allowed_paths,
                files: &t.files,
                initial_changes: &t.initial_changes,
                untracked_files: &t.untracked_files,
            }
        })
        .collect();
    std::fs::write(
        workspace.join("development.json"),
        serde_json::to_vec_pretty(&tasks)?,
    )?;
    std::fs::write(
        workspace.join("baseline-pack.json"),
        serde_json::to_vec_pretty(&batch.request.full_pack)?,
    )?;
    let context = GenerationContext {
        id,
        batch_id: batch_id.into(),
        workspace: workspace.to_string_lossy().into_owned(),
        task_ids: tasks.iter().map(|t| t.id.to_owned()).collect(),
        material_fingerprint: super::fingerprint(&workspace)?,
        tainted: false,
        eligible_for_generation: true,
    };
    db.conn().execute("INSERT INTO evaluation_generation_contexts(id,batch_id,context_json) VALUES (?1,?2,?3)",rusqlite::params![context.id,batch_id,serde_json::to_string(&context)?]).map_err(err)?;
    Ok(context)
}

pub(crate) fn read(db: &Db, id: &str) -> io::Result<GenerationContext> {
    let started = std::time::Instant::now();
    let json: String = db
        .conn()
        .query_row(
            "SELECT context_json FROM evaluation_generation_contexts WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let mut context: GenerationContext = serde_json::from_str(&json)?;
    if context.id != id {
        return Err(err("generation context identity mismatch"));
    }
    context.tainted = db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_disclosures WHERE context_id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    context.eligible_for_generation = !context.tainted
        && super::fingerprint(Path::new(&context.workspace))
            .is_ok_and(|digest| digest == context.material_fingerprint);
    crate::diag::note(
        if context.eligible_for_generation {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !context.eligible_for_generation,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_generation",
        if context.eligible_for_generation {
            "isolated_inputs"
        } else if context.tainted {
            "heldout_context_tainted"
        } else {
            "generation_material_changed"
        },
        started,
    );
    Ok(context)
}

/// Explicitly giving heldout feedback to a generator is irreversible for this
/// context and these task contents. New names/batches do not erase the ledger.
pub(crate) fn reveal(db: &Db, id: &str, task_id: &str) -> io::Result<GenerationContext> {
    let started = std::time::Instant::now();
    let context = read(db, id)?;
    let batch = config::read(db, &context.batch_id)?;
    let case = batch
        .request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == task_id && c.split == "heldout")
        .ok_or_else(|| {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(crate::PROJECT_ID),
                None,
                None,
                None,
                "evaluation_heldout",
                "invalid_disclosure_target",
                started,
            );
            err("not a heldout task in this generation context")
        })?;
    db.conn().execute("INSERT OR IGNORE INTO evaluation_disclosures(context_id,task_key,task_id,batch_id) VALUES (?1,?2,?3,?4)",rusqlite::params![id,task_identity(&case.task)?,task_id,context.batch_id]).map_err(err)?;
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_heldout",
        "revealed_to_generator",
        started,
    );
    read(db, id)
}

pub(crate) fn store_check(db: &Db, report: &IsolationReport) -> io::Result<()> {
    db.conn()
        .execute(
            "INSERT INTO evaluation_isolation_checks(id,report_json) VALUES (?1,?2)",
            rusqlite::params![report.id, serde_json::to_string(report)?],
        )
        .map_err(err)?;
    Ok(())
}

pub(crate) fn read_check(db: &Db, id: &str) -> io::Result<IsolationReport> {
    let json: String = db
        .conn()
        .query_row(
            "SELECT report_json FROM evaluation_isolation_checks WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let report: IsolationReport = serde_json::from_str(&json)?;
    if report.id != id || report.passed != passed(&report) {
        return Err(err("damaged isolation check"));
    }
    Ok(report)
}

pub(crate) fn record_use(db: &Db, batch: &str, run: &str, task: &EvaluationTask) -> io::Result<()> {
    db.conn().execute("INSERT INTO evaluation_task_uses(run_id,batch_id,task_key,task_id) VALUES (?1,?2,?3,?4)",rusqlite::params![run,batch,task_identity(task)?,task.id]).map_err(err)?;
    Ok(())
}

pub(crate) fn passed(report: &IsolationReport) -> bool {
    // A missing/unproven boundary is not isolation. False rejection postpones
    // evaluation; false acceptance exposes hidden answers to the executor.
    report.public_requirements_readable
        && report.filesystem_denied
        && report.terminal_denied
        && report.child_process_denied
        && report.index_clean
        && report.messages_clean
        && report.host_material_unchanged
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn missing_boundary_evidence_never_verifies(index in 0usize..7) {
            let mut facts=[true;7];facts[index]=false;
            let report=IsolationReport {id:"test".into(),runtime_fingerprint:"test".into(),passed:true,public_requirements_readable:facts[0],filesystem_denied:facts[1],terminal_denied:facts[2],child_process_denied:facts[3],index_clean:facts[4],messages_clean:facts[5],host_material_unchanged:facts[6],evidence_kind:"host_boundary_probe".into()};
            prop_assert!(!passed(&report));
        }
        #[test]
        fn metadata_changes_do_not_reset_task_identity(name in "[a-z]{1,24}",revision in "[a-z0-9]{1,24}") {
            let corpus: super::super::EvaluationCorpus=serde_json::from_str(include_str!("../../../../evaluation/corpus/bugs.json")).unwrap();
            let mut task=corpus.cases[3].task.clone();
            let key=task_identity(&task).unwrap();
            task.id=name.clone();task.source=name.clone();task.license=name.clone();task.revision=revision;task.dependencies=vec![name.clone()];task.allowed_paths.reverse();task.category=name;
            prop_assert_eq!(task_identity(&task).unwrap(),key.clone());
            task.requirements.push_str(" Different behavior.");
            prop_assert_ne!(task_identity(&task).unwrap(),key);
        }
    }
}
