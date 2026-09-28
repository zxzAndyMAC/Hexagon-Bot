use super::*;
use serde_json::json;

pub(super) fn request() -> crate::evaluation::FreezeRequest {
    let corpora: Vec<serde_json::Value> = [
        include_str!("../../../../evaluation/corpus/bugs.json"),
        include_str!("../../../../evaluation/corpus/features.json"),
        include_str!("../../../../evaluation/corpus/interfaces.json"),
        include_str!("../../../../evaluation/corpus/dirty-trees.json"),
    ]
    .into_iter()
    .map(|s| serde_json::from_str(s).unwrap())
    .collect();
    serde_json::from_value(json!({
        "corpora":corpora,
        "main_slot":"default",
        "fast_role":"后端",
        "task_owners":["后端"],
        "full_pack":{"name":"evaluation-existing-role", "version":1,
            "stages":[{"name":"delivery","roles":["后端"],"due":[],"stamp_point":true}]},
        "prices":{},
        "limits":{"total_mc":20000000,"pilot_mc":2000000,"run_mc":500000,"requests":80,"active_ms":1800000},
        "statistics_version":"paired-benefit-v1"
    })).unwrap()
}

#[test]
fn evaluation_frozen_batch_survives_reopen_without_claiming_model_readiness() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = wb.freeze_evaluation(&request(), None).unwrap();
    assert!(!batch.ready);
    assert!(batch
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::ModelNotVerified));
    assert!(batch
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::PriceNotVerified));
    let observation = crate::evaluation::VerificationObservation {
        dimension: crate::evaluation::VerificationDimension::Model,
        outcome: crate::evaluation::ObservedOutcome::ReportedPass,
        batch_fingerprint: batch.fingerprint.clone(),
        source_url: "https://provider.example/models".into(),
        checked_at: "2026-09-27".into(),
    };
    let observed = wb
        .record_evaluation_verification(&batch.id, &observation)
        .unwrap();
    assert!(!observed.ready);
    assert!(observed
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::ModelNotVerified));
    let identity = batch.id.clone();
    drop(wb);
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let restored = wb.evaluation_batch(&identity).unwrap();
    assert_eq!(restored.fingerprint, batch.fingerprint);
    assert_eq!(restored.verification.observations.len(), 1);
    let mut foreign = observation;
    foreign.batch_fingerprint = "foreign-batch".into();
    assert!(wb
        .record_evaluation_verification(&identity, &foreign)
        .is_err());
    assert_eq!(
        serde_json::to_value(restored.request).unwrap(),
        serde_json::to_value(request()).unwrap()
    );
}

#[test]
fn evaluation_config_changes_require_a_linked_new_batch() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let original = request();
    let batch = wb.freeze_evaluation(&original, None).unwrap();
    assert!(batch.runtime.roles.contains_key("后端技术负责人"));
    // Ticket 06 review regression: implicit reviewer changes were omitted.
    wb.db.conn().execute("INSERT INTO role_defs(project_id,name,duty,model_slot,skills,custom) VALUES (?1,'后端技术负责人','changed reviewer','different-slot','[]',0)", [&wb.project_id]).unwrap();
    let drift = wb
        .check_evaluation_configuration(&batch.id, &original)
        .unwrap();
    assert!(drift
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::RuntimeDrift));
    // An invalid reviewer chain is also a structured refusal, not an opaque error.
    wb.db
        .conn()
        .execute(
            "UPDATE role_defs SET reviewer='missing reviewer' WHERE project_id=?1",
            [&wb.project_id],
        )
        .unwrap();
    let missing = wb
        .check_evaluation_configuration(&batch.id, &original)
        .unwrap();
    assert!(missing
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::RuntimeDrift));
    wb.db
        .conn()
        .execute(
            "DELETE FROM role_defs WHERE project_id=?1",
            [&wb.project_id],
        )
        .unwrap();
    let mut changed = original.clone();
    changed.full_pack.stages[0].stamp_point = false;
    let check = wb
        .check_evaluation_configuration(&batch.id, &changed)
        .unwrap();
    assert!(check
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::ConfigurationDrift));
    let next = wb.freeze_evaluation(&changed, Some(&batch.id)).unwrap();
    assert_ne!(next.id, batch.id);
    assert_eq!(next.parent_batch.as_deref(), Some(batch.id.as_str()));
    assert_ne!(next.fingerprint, batch.fingerprint);
    assert_eq!(
        wb.evaluation_batch(&batch.id).unwrap().fingerprint,
        batch.fingerprint
    );
}

#[test]
fn evaluation_freeze_refuses_missing_safety_contract_before_paid_preflight() {
    // 2026-09-28 pilot eval-3: old corpora admitted paid work that could never
    // produce a formal safety verdict. Standalone debug still supports legacy tasks.
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = request();
    request.corpora[0].cases[0].task.safety = None;
    assert!(wb.freeze_evaluation(&request, None).is_err());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(24))]
    #[test]
    fn evaluation_every_frozen_task_requires_safety(category in 0usize..4, case in 0usize..5, missing in proptest::bool::ANY) {
        let home = tempfile::tempdir().unwrap();
        let wb = Workbench::open_evaluation_host(home.path()).unwrap();
        let mut request = request();
        let safety = &mut request.corpora[category].cases[case].task.safety;
        if missing { *safety = None; } else { safety.as_mut().unwrap().scope_reason = " \n".into(); }
        proptest::prop_assert!(wb.freeze_evaluation(&request, None).is_err());
    }
}
