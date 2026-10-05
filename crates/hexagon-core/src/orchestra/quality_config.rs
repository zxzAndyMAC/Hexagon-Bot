//! Q4 owner decision 2026-10-05: confirmed quality commands amend the running
//! flow. Keep the pinned process/artifact history; bind checks and exceptions
//! to monotonic category revisions so A→B→A cannot resurrect old consent.
use super::*;
use std::collections::BTreeMap;

const KIND: &str = "quality_commands_changed";
type Commands = BTreeMap<QualityCategory, String>;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct QualityStageConfiguration {
    #[ts(type = "number")]
    pub seq: usize,
    pub name: String,
    pub commands: Commands,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct QualityConfiguration {
    pub version: String,
    pub project_root: String,
    pub stages: Vec<QualityStageConfiguration>,
}

#[derive(Deserialize)]
struct Change {
    seq: usize,
    commands: Commands,
    changed_categories: Vec<QualityCategory>,
    previous: Commands,
    original_checks: Vec<String>,
}

fn changes(db: &Db, project: &str) -> Result<Vec<(i64, Change)>, OrchError> {
    let mut query = db.conn().prepare("SELECT id,payload FROM events WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.kind')=?2 ORDER BY id")?;
    let rows = query.query_map(rusqlite::params![project, KIND], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (id, body) = row?;
        Ok((id, serde_json::from_str(&body)?))
    })
    .collect()
}

fn hash(value: &impl Serialize) -> Result<String, OrchError> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "qc1:{:x}",
        Sha256::digest(serde_json::to_vec(value)?)
    ))
}

pub fn effective_pack(db: &Db, project: &str, base: &PackDef) -> Result<PackDef, OrchError> {
    let history = changes(db, project)?;
    project_pack(base, &history)
}

fn project_pack(base: &PackDef, history: &[(i64, Change)]) -> Result<PackDef, OrchError> {
    let mut pack = original_pack(base, history)?;
    let mut latest = BTreeMap::new();
    for (_, change) in history {
        latest.insert(change.seq, change);
    }
    for (seq, change) in latest {
        let stage = pack
            .stages
            .get_mut(seq)
            .ok_or(OrchError::BadSeq(seq as i64))?;
        // Some packs list the same quality runner as an ordinary check. Replace
        // that occurrence too; leaving it behind kept executing the old runner.
        let old = stage.quality_checks.clone();
        stage.checks = stage
            .checks
            .iter()
            .flat_map(|command| {
                let matches: Vec<_> = old.iter().filter(|(_, value)| *value == command).collect();
                if matches.is_empty() {
                    return vec![command.clone()];
                }
                matches
                    .into_iter()
                    .filter_map(|(category, _)| change.commands.get(category).cloned())
                    .collect()
            })
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        stage.checks.retain(|command| seen.insert(command.clone()));
        stage.quality_checks = change.commands.clone();
    }
    Ok(pack)
}

fn original_pack(base: &PackDef, history: &[(i64, Change)]) -> Result<PackDef, OrchError> {
    let mut pack = base.clone();
    let mut seen = std::collections::BTreeSet::new();
    for (_, change) in history {
        if !seen.insert(change.seq) {
            continue;
        }
        let stage = pack
            .stages
            .get_mut(change.seq)
            .ok_or(OrchError::BadSeq(change.seq as i64))?;
        stage.checks = change.original_checks.clone();
        stage.quality_checks = change.previous.clone();
    }
    Ok(pack)
}

pub fn quality_configuration(
    db: &Db,
    project: &str,
    base: &PackDef,
) -> Result<QualityConfiguration, OrchError> {
    let history = changes(db, project)?;
    // QA review 2026-10-05: one snapshot must supply both commands and CAS
    // version. Two reads could pair old commands with a newer write's version.
    let pack = project_pack(base, &history)?;
    let revisions: Vec<_> = history.iter().map(|(id, _)| *id).collect();
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    let original = original_pack(base, &history)?;
    Ok(QualityConfiguration {
        version: hash(&(project, &root, original, revisions))?,
        project_root: root,
        stages: pack
            .stages
            .iter()
            .enumerate()
            .map(|(seq, stage)| QualityStageConfiguration {
                seq,
                name: stage.name.clone(),
                commands: stage.quality_checks.clone(),
            })
            .collect(),
    })
}

pub fn update_quality_commands(
    db: &Db,
    project: &str,
    base: &PackDef,
    seq: usize,
    expected: &str,
    commands: &Commands,
) -> Result<QualityConfiguration, OrchError> {
    let started = std::time::Instant::now();
    let result = (|| {
        let _lease = write_boundary(db, project)?;
        let tx = db.conn().unchecked_transaction()?;
        let current = quality_configuration(db, project, base)?;
        if expected.is_empty() || current.version != expected {
            return Err(OrchError::StaleQualityConfiguration);
        }
        let old = &current
            .stages
            .get(seq)
            .ok_or(OrchError::BadSeq(seq as i64))?
            .commands;
        let commands: Commands = commands
            .iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(category, value)| (category.clone(), value.trim().to_string()))
            .collect();
        if commands
            .values()
            .any(|value| value.contains('\0') || value.len() > 65536)
        {
            return Err(OrchError::InvalidQualityCommand);
        }
        let changed_categories: Vec<_> = [
            QualityCategory::Tests,
            QualityCategory::Performance,
            QualityCategory::Accessibility,
            QualityCategory::Security,
        ]
        .into_iter()
        .filter(|category| old.get(category) != commands.get(category))
        .collect();
        if !changed_categories.is_empty() {
            let original_checks = effective_pack(db, project, base)?.stages[seq]
                .checks
                .clone();
            db.append_event(project, EventKind::System, json!({"kind":KIND,"by":"owner","seq":seq,"previous":old,"commands":commands,"original_checks":original_checks,"changed_categories":changed_categories}),None,None)?;
        }
        let updated = quality_configuration(db, project, base)?;
        if !changed_categories.is_empty() && db.active_stage_run(project)?.is_none() {
            let pack = effective_pack(db, project, base)?;
            if let Some(run) = acceptance_run(db, project, &pack)? {
                request_revalidation(db, project, &run)?;
            }
        }
        tx.commit()?;
        Ok(updated)
    })();
    let code = match &result {
        Ok(_) => "confirmed",
        Err(OrchError::StaleQualityConfiguration) => "stale_version",
        Err(OrchError::InvalidQualityCommand) => "invalid_command",
        Err(_) => "boundary_refused",
    };
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(project),
        Some("owner"),
        None,
        None,
        "quality_commands_confirm",
        code,
        started,
    );
    result
}

pub(super) fn request_revalidation(
    db: &Db,
    project: &str,
    run: &StageRun,
) -> Result<String, OrchError> {
    if let Some(card) = db.queued_questions(project)?.into_iter().find(|card| {
        card.payload["sub"] == "quality_revalidation" && card.payload["run_id"] == run.id
    }) {
        return Ok(card.id);
    }
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    Ok(crate::cards::enqueue(
        db,
        project,
        None,
        crate::cards::CardKind::Stamp,
        json!({"sub":"quality_revalidation","project_root":root,"run_id":run.id,"stage":run.stage_name,"seq":run.seq,"final_acceptance":true}),
        None,
    )?)
}

pub fn cancel_quality_revalidation(
    db: &Db,
    project: &str,
    question: &str,
) -> Result<(), OrchError> {
    let started = std::time::Instant::now();
    let run = crate::cards::get(db, question)
        .ok()
        .filter(|card| card.project_id == project)
        .and_then(|card| card.payload["run_id"].as_str().map(str::to_owned));
    let result = cancel_revalidation_inner(db, project, question);
    crate::diag::note(
        if result.is_ok() {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        result.is_err(),
        Some(project),
        Some("owner"),
        run.as_deref(),
        None,
        "quality_revalidation_cancel",
        if result.is_ok() {
            "cancelled"
        } else {
            "boundary_refused"
        },
        started,
    );
    result
}

fn cancel_revalidation_inner(db: &Db, project: &str, question: &str) -> Result<(), OrchError> {
    let _lease = write_boundary(db, project)?;
    let tx = db.conn().unchecked_transaction()?;
    let card = crate::cards::get(db, question)?;
    if card.project_id != project
        || card.kind != "stamp"
        || card.payload["sub"] != "quality_revalidation"
    {
        return Err(OrchError::InvalidAcceptanceException);
    }
    if card.state == crate::cards::CardState::Answered {
        return Ok(());
    }
    crate::cards::get_queued(db, question, crate::cards::CardKind::Stamp)?;
    crate::cards::answer(db, question, "owner")?;
    db.append_event(
        project,
        EventKind::PermissionDenied,
        json!({"kind":"quality_revalidation","question_id":question,"by":"owner"}),
        None,
        card.payload["run_id"].as_str(),
    )?;
    tx.commit()?;
    Ok(())
}

/// Selection and its prefix are bound: changing an unused later fallback does
/// not expire this check, while removing the chosen runner does. False negative
/// costs one recheck; false positive could reuse unapproved command evidence.
pub(super) fn check_binding(
    db: &Db,
    project: &str,
    pack: &PackDef,
    run: &str,
    command: &str,
) -> Result<String, OrchError> {
    let seq: i64 = db.conn().query_row(
        "SELECT seq FROM stage_runs WHERE project_id=?1 AND id=?2",
        rusqlite::params![project, run],
        |r| r.get(0),
    )?;
    let seq = usize::try_from(seq)
        .ok()
        .filter(|seq| *seq < pack.stages.len())
        .ok_or(OrchError::BadSeq(seq))?;
    let mut revisions = BTreeMap::new();
    for (id, change) in changes(db, project)? {
        for category in change.changed_categories {
            revisions.insert((change.seq, category), id);
        }
    }
    let categories: Vec<_> = [
        QualityCategory::Tests,
        QualityCategory::Performance,
        QualityCategory::Accessibility,
        QualityCategory::Security,
    ]
    .into_iter()
    .filter(|category| {
        command == format!("quality:{}", category.key())
            || pack
                .stages
                .get(seq)
                .and_then(|s| s.quality_checks.get(category))
                .is_some_and(|c| c == command)
    })
    .collect();
    let mut inputs = Vec::new();
    for category in categories {
        let mut prefix = Vec::new();
        for index in
            std::iter::once(seq).chain((0..pack.stages.len()).filter(|index| *index != seq))
        {
            let value = pack.stages[index]
                .quality_checks
                .get(&category)
                .filter(|c| !c.trim().is_empty());
            prefix.push((
                index,
                revisions
                    .get(&(index, category.clone()))
                    .copied()
                    .unwrap_or(0),
                value.cloned(),
            ));
            if value.is_some() {
                break;
            }
        }
        inputs.push((category, prefix));
    }
    if inputs
        .iter()
        .all(|(_, prefix)| prefix.iter().all(|(_, revision, _)| *revision == 0))
    {
        return Ok("legacy".into());
    }
    hash(&inputs)
}

pub(super) fn binding_current(recorded: Option<&str>, current: &str) -> bool {
    recorded.unwrap_or("legacy") == current
}

pub(super) fn has_unaccepted_change(db: &Db, project: &str) -> Result<bool, OrchError> {
    let accepted: i64 = db.conn().query_row("SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.kind')='quality_source_baseline' AND json_extract(payload,'$.reason')='accepted_version'", [project], |r| r.get(0))?;
    Ok(changes(db, project)?.iter().any(|(id, _)| *id > accepted))
}

pub(super) fn unaccepted_categories(
    db: &Db,
    project: &str,
    pack: &PackDef,
    seq: usize,
) -> Result<Vec<QualityCategory>, OrchError> {
    let accepted: i64 = db.conn().query_row("SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.kind')='quality_source_baseline' AND json_extract(payload,'$.reason')='accepted_version'", [project], |r| r.get(0))?;
    let history = changes(db, project)?;
    let mut out = Vec::new();
    for category in [
        QualityCategory::Tests,
        QualityCategory::Performance,
        QualityCategory::Accessibility,
        QualityCategory::Security,
    ] {
        for index in
            std::iter::once(seq).chain((0..pack.stages.len()).filter(|index| *index != seq))
        {
            if history.iter().any(|(id, change)| {
                *id > accepted
                    && change.seq == index
                    && change.changed_categories.contains(&category)
            }) {
                out.push(category.clone());
                break;
            }
            if pack.stages[index]
                .quality_checks
                .get(&category)
                .is_some_and(|value| !value.trim().is_empty())
            {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        #[test]
        fn changed_historical_quality_contracts_keep_their_own_alias(category in 0usize..4, completed in any::<bool>()) {
            let category = [QualityCategory::Tests, QualityCategory::Performance, QualityCategory::Accessibility, QualityCategory::Security][category].clone();
            let dir = tempfile::tempdir().unwrap();
            let pack: PackDef = serde_json::from_value(json!({"name":"historical property","version":1,"stages":[
                {"name":"build","roles":["dev"],"due":[],"checks":[],"quality_checks":{(category.key()):"old"},"stamp_point":false},
                {"name":"delivery","roles":["dev"],"due":[],"quality_checks":{(category.key()):"later"},"stamp_point":true}
            ]})).unwrap();
            let wb = crate::api::Workbench::for_test(dir.path(), &["dev"], Some(pack.clone())).unwrap();
            let earlier = wb.open_stage(0).unwrap().run_id;
            wb.db.conn().execute("UPDATE stage_runs SET state='done' WHERE id=?1", [&earlier]).unwrap();
            let last = wb.open_stage(1).unwrap().run_id;
            if completed { wb.db.conn().execute("UPDATE stage_runs SET state='done' WHERE id=?1", [&last]).unwrap(); }
            let config = wb.quality_configuration().unwrap();
            wb.update_quality_commands(0, &config.version, &Commands::from([(category.clone(),"new".into())])).unwrap();
            let effective = effective_pack(&wb.db, &wb.project_id, &pack).unwrap();
            let requirements = quality::requirements(&wb.db, &wb.project_id, &earlier, &effective.stages[0], &effective).unwrap();
            prop_assert_eq!(requirements.len(), 1);
            prop_assert_eq!(requirements[0].category, category.key());
            prop_assert_eq!(requirements[0].command.as_deref(), Some("new"));
            prop_assert!(requirements[0].paths.is_empty());
            prop_assert!(quality::requirements(&wb.db, &wb.project_id, &last, &effective.stages[1], &effective).unwrap().is_empty());
        }
        #[test]
        fn completed_evaluation_never_hides_a_failed_middle_behind_an_absent_tail(index in 0usize..4) {
            let dir = tempfile::tempdir().unwrap();
            let pack: PackDef = serde_json::from_value(json!({"name":"tail property","version":1,"stages":[{"name":"build","roles":["dev"],"due":[],"stamp_point":false},{"name":"checks","roles":["dev"],"due":[],"stamp_point":false},{"name":"tail","roles":["absent"],"due":[],"stamp_point":false}]})).unwrap();
            let wb = crate::api::Workbench::for_test(dir.path(), &["dev"], Some(pack.clone())).unwrap();
            let first = wb.open_stage(0).unwrap().run_id;
            wb.db.conn().execute("UPDATE stage_runs SET state='done' WHERE id=?1", [&first]).unwrap();
            let middle = wb.open_stage(1).unwrap().run_id;
            let state = ["done","skipped","rejected","interrupted"][index];
            wb.db.conn().execute("UPDATE stage_runs SET state=?1 WHERE id=?2", rusqlite::params![state,middle]).unwrap();
            wb.open_stage(2).unwrap();
            let verdict = evaluate(&wb.db, &wb.project_id, &pack).unwrap();
            prop_assert_eq!(matches!(verdict, StageEval::Ready), state == "done");
        }
        #[test]
        fn completed_target_respects_latest_attempt_and_the_actual_end_of_flow(index in 0usize..6, stamped in any::<bool>(), interrupted in any::<bool>()) {
            let dir = tempfile::tempdir().unwrap();
            let pack: PackDef = serde_json::from_value(json!({"name":"terminal property","version":1,"stages":[{"name":"build","roles":["dev"],"due":[],"stamp_point":stamped},{"name":"delivery","roles":["dev"],"due":[],"stamp_point":stamped}]})).unwrap();
            let wb = crate::api::Workbench::for_test(dir.path(), &["dev"], Some(pack.clone())).unwrap();
            let old = wb.open_stage(0).unwrap().run_id;
            wb.db.conn().execute("UPDATE stage_runs SET state='done' WHERE id=?1", [&old]).unwrap();
            prop_assert!(acceptance_run(&wb.db, &wb.project_id, &pack).unwrap().is_none(), "an unfinished flow cannot borrow a completed stage");
            let latest = wb.open_stage(1).unwrap().run_id;
            let state = ["active","waiting_stamp","done","skipped","rejected","interrupted"][index];
            wb.db.conn().execute("UPDATE stage_runs SET state=?1 WHERE id=?2", rusqlite::params![state,latest]).unwrap();
            let target = acceptance_run(&wb.db, &wb.project_id, &pack).unwrap();
            prop_assert_eq!(target.is_some(), index < 4);
            if state == "skipped" { prop_assert_eq!(target.unwrap().id, old); }
            if state == "done" || state == "skipped" {
                let newer = wb.open_stage(0).unwrap().run_id;
                wb.db.conn().execute("UPDATE stage_runs SET state=?1 WHERE id=?2", rusqlite::params![if interrupted {"interrupted"} else {"rejected"},newer]).unwrap();
                prop_assert!(acceptance_run(&wb.db, &wb.project_id, &pack).unwrap().is_none(), "a new failed attempt cannot borrow prior delivery");
            }
        }
        #[test]
        fn obligations_follow_the_selected_runner_prefix_including_removal(a in "[a-z]{1,12}", b in "[a-z]{1,12}") {
            prop_assume!(a != b);
            let dir = tempfile::tempdir().unwrap();
            let pack: PackDef = serde_json::from_value(json!({"name":"prefix property","version":1,"stages":[{"name":"build","roles":["dev"],"due":[],"quality_checks":{"tests":a},"stamp_point":false},{"name":"checks","roles":["dev"],"due":[],"quality_checks":{"tests":"old-fallback"},"stamp_point":false},{"name":"delivery","roles":["dev"],"due":[],"stamp_point":true}]})).unwrap();
            let wb = crate::api::Workbench::for_test(dir.path(), &["dev"], Some(pack.clone())).unwrap();
            let run = wb.open_stage(0).unwrap().run_id;
            let old_binding = check_binding(&wb.db, &wb.project_id, &pack, &run, "quality:tests").unwrap();
            let current = wb.quality_configuration().unwrap();
            let changed = wb.update_quality_commands(1, &current.version, &Commands::from([(QualityCategory::Tests,b.clone())])).unwrap();
            let effective = effective_pack(&wb.db, &wb.project_id, &pack).unwrap();
            prop_assert!(unaccepted_categories(&wb.db, &wb.project_id, &effective, 0).unwrap().is_empty());
            prop_assert_eq!(check_binding(&wb.db, &wb.project_id, &effective, &run, "quality:tests").unwrap(), old_binding);
            let removed = wb.update_quality_commands(0, &changed.version, &Commands::new()).unwrap();
            let effective = effective_pack(&wb.db, &wb.project_id, &pack).unwrap();
            let requirements = quality::requirements(&wb.db, &wb.project_id, &run, &effective.stages[0], &effective).unwrap();
            prop_assert!(requirements.iter().any(|r| r.category == "tests" && r.command.as_deref() == Some(b.as_str())));
            wb.update_quality_commands(1, &removed.version, &Commands::new()).unwrap();
            let effective = effective_pack(&wb.db, &wb.project_id, &pack).unwrap();
            let requirements = quality::requirements(&wb.db, &wb.project_id, &run, &effective.stages[0], &effective).unwrap();
            prop_assert!(requirements.iter().any(|r| r.category == "tests" && r.command.is_none()));
        }
        #[test]
        fn real_confirmation_roundtrip_never_revives_an_old_category_binding(a in "[a-z]{1,16}", b in "[a-z]{1,16}") {
            prop_assume!(a != b);
            let dir = tempfile::tempdir().unwrap();
            let pack: PackDef = serde_json::from_value(json!({"name":"property","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":["independent"],"quality_checks":{"tests":a},"stamp_point":true}]})).unwrap();
            let wb = crate::api::Workbench::for_test(dir.path(), &["dev"], Some(pack.clone())).unwrap();
            let run = wb.open_stage(0).unwrap().run_id;
            let old = check_binding(&wb.db, &wb.project_id, &pack, &run, "quality:tests").unwrap();
            let current = wb.quality_configuration().unwrap();
            let next = wb.update_quality_commands(0, &current.version, &Commands::from([(QualityCategory::Tests,b)])).unwrap();
            let returned = wb.update_quality_commands(0, &next.version, &Commands::from([(QualityCategory::Tests,a)])).unwrap();
            let effective = effective_pack(&wb.db, &wb.project_id, &pack).unwrap();
            let binding = check_binding(&wb.db, &wb.project_id, &effective, &run, "quality:tests").unwrap();
            prop_assert!(!binding_current(Some(&old), &binding));
            prop_assert_ne!(current.version, returned.version);
            prop_assert_eq!(&effective.stages[0].checks, &vec!["independent".to_string()]);
            prop_assert_eq!(serde_json::to_value(&effective).unwrap(), serde_json::to_value(effective_pack(&wb.db, &wb.project_id, &effective).unwrap()).unwrap());
        }
        #[test]
        fn old_or_missing_evidence_never_matches_a_new_revision(revision in 1i64..i64::MAX, command in "[a-z]{1,24}") {
            let old = hash(&(command.clone(), 0)).unwrap();
            let current = hash(&(command, revision)).unwrap();
            prop_assert!(!binding_current(Some(&old), &current));
            prop_assert!(!binding_current(None, &current));
            prop_assert!(binding_current(Some(&current), &current));
        }
    }
}
