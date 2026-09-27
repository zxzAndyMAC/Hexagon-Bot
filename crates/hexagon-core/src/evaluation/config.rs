//! Evaluation 06: immutable host snapshots. Configuration is not verification.
use super::{err, EvaluationCorpus};
use crate::{db::Db, orchestra::PackDef, presets::RoleDef};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    time::Instant,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationLimits {
    pub total_mc: u64,
    pub pilot_mc: u64,
    pub run_mc: u64,
    pub requests: u32,
    pub active_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceSource {
    pub provider_id: String,
    pub model: String,
    pub prompt_per_1k_mc: u64,
    pub completion_per_1k_mc: u64,
    pub source_url: String,
    pub checked_at: String,
    /// All separately billed dimensions must be addressed by the later probe.
    pub billing_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreezeRequest {
    pub corpora: Vec<EvaluationCorpus>,
    pub main_slot: String,
    pub fast_role: String,
    /// Owner assigns only the declared task files to these existing roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_owners: Vec<String>,
    pub full_pack: PackDef,
    pub prices: BTreeMap<String, PriceSource>,
    pub limits: EvaluationLimits,
    pub statistics_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionBlock {
    ConfigurationDrift,
    RuntimeDrift,
    HeldoutRetired,
    ModelNotConfigured,
    ModelNotVerified,
    ToolsNotVerified,
    PriceNotVerified,
    EnvironmentNotVerified,
    ExecutionGuardsPending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSnapshot {
    pub provider_id: String,
    pub model: String,
    pub kind: crate::provider::ProviderKind,
    /// Never retain URL userinfo/query credentials in a frozen identity.
    pub endpoint: Option<String>,
    pub enabled: bool,
    pub context_window: Option<u64>,
    pub max_output: Option<u64>,
    pub temperature: String,
    pub caps_hint: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub models: BTreeMap<String, ModelSnapshot>,
    pub roles: BTreeMap<String, RoleDef>,
    pub os: String,
    pub architecture: String,
    pub executable_fingerprint: String,
    pub runtime_versions: BTreeMap<String, Option<String>>,
    pub host_policy: HostPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supplement_statuses: Vec<u16>,
    pub reviewer_mode: String,
    pub permission_contract: String,
    pub worker_network: String,
    pub inherited_credentials: bool,
    pub inherited_mcp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenBatch {
    pub version: u32,
    pub id: String,
    pub parent_batch: Option<String>,
    pub request: FreezeRequest,
    pub runtime: RuntimeSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationBatch {
    pub id: String,
    pub parent_batch: Option<String>,
    pub fingerprint: String,
    pub request: FreezeRequest,
    pub runtime: RuntimeSnapshot,
    pub ready: bool,
    pub blocks: Vec<AdmissionBlock>,
    pub verification: Verification,
}

/// These facts stay unknown until metered preflight produces host receipts.
/// Capability hints from a provider directory never populate these fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Verification {
    pub model_request_id: Option<String>,
    pub tools_request_id: Option<String>,
    pub price_receipt: Option<String>,
    pub failure: Option<String>,
    #[serde(default)]
    pub observations: Vec<VerificationObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationDimension {
    Model,
    Tools,
    Price,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedOutcome {
    Unknown,
    Failed,
    ReportedPass,
}

/// Owner observations are retained separately from metered host receipts.
/// A reported pass alone cannot admit paid execution (D03/D08, ticket 06).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationObservation {
    pub dimension: VerificationDimension,
    pub outcome: ObservedOutcome,
    pub batch_fingerprint: String,
    pub source_url: String,
    pub checked_at: String,
}

pub(crate) fn digest<T: Serialize>(value: &T) -> io::Result<String> {
    Ok(format!(
        "v1:{:x}",
        Sha256::digest(serde_json::to_vec(value)?)
    ))
}

fn limits_valid(limits: &EvaluationLimits) -> bool {
    // D08: false rejection requires a corrected plan; false acceptance spends
    // beyond the approved round. New batches cannot raise these hard ceilings.
    limits.total_mc > 0
        && limits.total_mc <= 20_000_000
        && limits.pilot_mc > 0
        && limits.pilot_mc <= 2_000_000
        && limits.pilot_mc <= limits.total_mc
        && limits.run_mc > 0
        && limits.run_mc <= 500_000
        && limits.run_mc <= limits.pilot_mc
        && limits.requests > 0
        && limits.requests <= 80
        && limits.active_ms > 0
        && limits.active_ms <= 1_800_000
}

fn validate(request: &FreezeRequest) -> io::Result<()> {
    let mut categories = BTreeSet::new();
    let mut ids = BTreeSet::new();
    if request.corpora.len() != 4
        || !limits_valid(&request.limits)
        || request.main_slot.trim().is_empty()
        || request.fast_role.trim().is_empty()
        || request.full_pack.stages.is_empty()
        || request.statistics_version != "paired-benefit-v1"
    {
        return Err(err("invalid frozen evaluation configuration"));
    }
    for corpus in &request.corpora {
        if !super::category_valid(corpus) {
            return Err(err("invalid frozen task partition"));
        }
        categories.insert(&corpus.cases[0].task.category);
        for case in &corpus.cases {
            super::validate(&case.task)?;
            if !ids.insert(&case.task.id) {
                return Err(err("duplicate frozen task identity"));
            }
        }
    }
    if categories.len() != 4 {
        return Err(err("all four task categories are required"));
    }
    for price in request.prices.values() {
        if price.provider_id.trim().is_empty()
            || price.model.trim().is_empty()
            || public_url(&price.source_url).is_none()
            || price.checked_at.trim().is_empty()
            || price.billing_scope.trim().is_empty()
        {
            return Err(err(
                "price source is incomplete; supplied rates are not verified prices",
            ));
        }
    }
    Ok(())
}

fn public_url(text: &str) -> Option<String> {
    let url = url::Url::parse(text).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none())
    .then(|| url.to_string())
}

pub(crate) fn code_fingerprint() -> io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(std::env::current_exe()?)?;
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        digest.update(&bytes[..n]);
    }
    Ok(format!("v1:{:x}", digest.finalize()))
}

fn versions() -> io::Result<BTreeMap<String, Option<String>>> {
    let root = tempfile::tempdir()?;
    let db = Db::open_in_memory().map_err(err)?;
    let ctx = crate::tools::ToolContext::owner(&db, root.path());
    let python = "python=python3; if command -v xcode-select >/dev/null 2>&1; then developer=$(xcode-select -p); native=\"$developer/Library/Frameworks/Python3.framework/Versions/Current/Resources/Python.app/Contents/MacOS/Python\"; if test -x \"$native\"; then python=\"$native\"; fi; fi; \"$python\" --version";
    let mut versions = BTreeMap::new();
    for (name, command) in [
        ("python", python),
        ("node", "node --version"),
        ("git", "git --version"),
    ] {
        let result = crate::sessions::SessionTable::default()
            .run_oneshot(&db, &ctx, command, std::time::Duration::from_secs(5), false)
            .map_err(err)?;
        let version = (result["exit_code"].as_i64() == Some(0)
            && result["timed_out"].as_bool() == Some(false))
        .then(|| result["stdout"].as_str().unwrap_or("").trim().to_owned())
        .filter(|s| !s.is_empty());
        versions.insert(name.into(), version);
    }
    Ok(versions)
}

pub(crate) fn collect_roles(
    names: BTreeSet<String>,
    mut lookup: impl FnMut(&str) -> io::Result<RoleDef>,
) -> io::Result<BTreeMap<String, RoleDef>> {
    let mut roles = BTreeMap::new();
    for name in names {
        let mut next = Some(name);
        let mut chain = BTreeSet::new();
        while let Some(name) = next {
            // Ticket 06 review: an implicit superior can adjudicate proposals.
            // Omitting it made its configuration changes invisible to drift checks.
            if !chain.insert(name.clone()) {
                return Err(err("cyclic evaluation reviewer chain"));
            }
            if roles.contains_key(&name) {
                break;
            }
            let role = lookup(&name)?;
            next = role.reviewer.clone();
            roles.insert(name, role);
        }
    }
    Ok(roles)
}

fn snapshot(db: &Db, project: &str, request: &FreezeRequest) -> io::Result<RuntimeSnapshot> {
    let doc = crate::provider_config::load().map_err(err)?;
    let mut models = BTreeMap::new();
    for (slot, binding) in &doc.slots {
        let Some(def) = doc.providers.iter().find(|d| d.id == binding.provider_id) else {
            continue;
        };
        let provider = crate::provider_config::make_provider(
            def,
            &binding.model,
            std::sync::Arc::new(crate::credentials::MemoryStore::default()),
        );
        let meta = provider.model_meta();
        let mut caps = def
            .models
            .iter()
            .find(|m| m.id == binding.model)
            .map(|m| m.caps.clone())
            .unwrap_or_default();
        caps.sort();
        caps.dedup();
        models.insert(
            slot.clone(),
            ModelSnapshot {
                provider_id: def.id.clone(),
                model: binding.model.clone(),
                kind: def.kind.clone(),
                endpoint: public_url(&def.base_url),
                enabled: def.enabled,
                context_window: meta.context_window,
                max_output: provider.output_token_limit(),
                temperature: "provider_default_unspecified".into(),
                caps_hint: caps,
            },
        );
    }
    let mut names = BTreeSet::from([request.fast_role.clone()]);
    for stage in &request.full_pack.stages {
        names.extend(stage.roles.iter().cloned());
        names.extend(stage.reviews.iter().map(|r| r.reviewer.clone()));
        names.extend(stage.consult_wake.iter().cloned());
        for (from, to) in &stage.backfill_edges {
            names.insert(from.clone());
            names.insert(to.clone());
        }
    }
    let roles = collect_roles(names, |name| {
        crate::roles::role_def(db, project, name).map_err(err)
    })?;
    if request
        .task_owners
        .iter()
        .any(|name| !roles.contains_key(name))
    {
        return Err(err("task ownership must name an existing frozen role"));
    }
    Ok(RuntimeSnapshot {
        models,
        roles,
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        executable_fingerprint: code_fingerprint()?,
        runtime_versions: versions()?,
        host_policy: HostPolicy {
            supplement_statuses: vec![502, 503, 504],
            reviewer_mode: crate::autonomy::reviewer_mode(db, project).map_err(err)?,
            permission_contract: "existing-instance-permissions-v1".into(),
            worker_network: "denied-except-host-model-transport".into(),
            inherited_credentials: false,
            inherited_mcp: false,
        },
    })
}

pub(crate) fn observe(
    db: &Db,
    project: &str,
    id: &str,
    observation: &VerificationObservation,
) -> io::Result<EvaluationBatch> {
    let started = Instant::now();
    let batch = read(db, id)?;
    if observation.batch_fingerprint != batch.fingerprint
        || public_url(&observation.source_url).is_none()
        || observation.checked_at.trim().is_empty()
    {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(project),
            None,
            None,
            None,
            "evaluation_verification_observation",
            "invalid_observation_binding",
            started,
        );
        return Err(err(
            "verification observation must bind this batch and a non-secret source",
        ));
    }
    // Atomic append: concurrent owner observations must not overwrite each other.
    db.conn().execute(
        "UPDATE evaluation_batches SET verification_json=json_set(verification_json,'$.observations',json_insert(COALESCE(json_extract(verification_json,'$.observations'),'[]'),'$[#]',json(?2))) WHERE id=?1",
        rusqlite::params![id, serde_json::to_string(observation)?],
    ).map_err(err)?;
    read(db, id)
}

fn view(record: FrozenBatch, fingerprint: String, verification: Verification) -> EvaluationBatch {
    let mut blocks = vec![AdmissionBlock::ExecutionGuardsPending];
    if !record
        .runtime
        .models
        .get(&record.request.main_slot)
        .is_some_and(|m| m.enabled && m.endpoint.is_some() && m.max_output.is_some())
    {
        blocks.push(AdmissionBlock::ModelNotConfigured);
    }
    if verification.model_request_id.is_none() {
        blocks.push(AdmissionBlock::ModelNotVerified);
    }
    if verification.tools_request_id.is_none() {
        blocks.push(AdmissionBlock::ToolsNotVerified);
    }
    if verification.price_receipt.is_none() {
        blocks.push(AdmissionBlock::PriceNotVerified);
    }
    if record
        .runtime
        .runtime_versions
        .values()
        .any(Option::is_none)
    {
        blocks.push(AdmissionBlock::EnvironmentNotVerified);
    }
    EvaluationBatch {
        id: record.id,
        parent_batch: record.parent_batch,
        fingerprint,
        request: record.request,
        runtime: record.runtime,
        ready: blocks.is_empty(),
        blocks,
        verification,
    }
}

pub(crate) fn freeze(
    db: &Db,
    project: &str,
    request: &FreezeRequest,
    parent: Option<&str>,
) -> io::Result<EvaluationBatch> {
    let started = Instant::now();
    let result = (|| {
        validate(request)?;
        if let Some(parent) = parent {
            read(db, parent)?;
        }
        let record = FrozenBatch {
            version: 1,
            id: format!("batch-{}", db.next_id("evaluation_batch").map_err(err)?),
            parent_batch: parent.map(str::to_owned),
            request: request.clone(),
            runtime: snapshot(db, project, request)?,
        };
        let fingerprint = digest(&record)?;
        let verification = Verification::default();
        db.conn().execute("INSERT INTO evaluation_batches(id,parent_id,frozen_json,fingerprint,verification_json) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![record.id,record.parent_batch,serde_json::to_string(&record)?,fingerprint,serde_json::to_string(&verification)?]).map_err(err)?;
        with_retirement(db, view(record, fingerprint, verification))
    })();
    crate::diag::note(
        if result.is_ok() {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        result.is_err(),
        Some(project),
        None,
        None,
        None,
        "evaluation_freeze",
        if result.is_ok() {
            "frozen_unverified"
        } else {
            "invalid_configuration"
        },
        started,
    );
    result
}

pub(crate) fn read(db: &Db, id: &str) -> io::Result<EvaluationBatch> {
    let (json, fingerprint, verification): (String, String, String) = db
        .conn()
        .query_row(
            "SELECT frozen_json,fingerprint,verification_json FROM evaluation_batches WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(err)?;
    let record: FrozenBatch = serde_json::from_str(&json)?;
    if record.version != 1 || record.id != id || digest(&record)? != fingerprint {
        return Err(err("unsupported or damaged frozen evaluation batch"));
    }
    with_retirement(
        db,
        view(record, fingerprint, serde_json::from_str(&verification)?),
    )
}

fn with_retirement(db: &Db, mut batch: EvaluationBatch) -> io::Result<EvaluationBatch> {
    for case in batch
        .request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .filter(|c| c.split == "heldout")
    {
        if super::isolation::retired(db, &case.task)? {
            batch.blocks.push(AdmissionBlock::HeldoutRetired);
            batch.ready = false;
            break;
        }
    }
    Ok(batch)
}

pub(crate) fn check(
    db: &Db,
    project: &str,
    id: &str,
    request: &FreezeRequest,
) -> io::Result<EvaluationBatch> {
    let started = Instant::now();
    let mut batch = read(db, id)?;
    if digest(&batch.request)? != digest(request)? {
        batch.blocks.push(AdmissionBlock::ConfigurationDrift);
    }
    // Ticket 06 review: missing roles or damaged provider configuration are
    // runtime drift, not an escape from the structured admission response.
    let runtime_matches = snapshot(db, project, &batch.request)
        .and_then(|runtime| digest(&runtime))
        .is_ok_and(|current| digest(&batch.runtime).is_ok_and(|frozen| frozen == current));
    if !runtime_matches {
        batch.blocks.push(AdmissionBlock::RuntimeDrift);
    }
    batch.ready = batch.blocks.is_empty();
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(project),
        None,
        None,
        None,
        "evaluation_admission",
        "not_verified_or_drifted",
        started,
    );
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn increasing_round_budget_beyond_approved_cap_never_admits(extra in 1u64..1_000_000) {
            let mut limits = EvaluationLimits { total_mc: 20_000_000, pilot_mc: 2_000_000, run_mc: 500_000, requests: 80, active_ms: 1_800_000 };
            prop_assert!(limits_valid(&limits));
            limits.total_mc += extra;
            prop_assert!(!limits_valid(&limits));
        }
        #[test]
        fn circular_review_chains_never_freeze(length in 1usize..12) {
            let result = collect_roles(BTreeSet::from(["0".into()]), |name| {
                let i: usize = name.parse().unwrap();
                Ok(RoleDef { name: name.into(), duty: "review".into(), reviewer: Some(((i + 1) % length).to_string()), model_slot: "chat".into(), globs: vec![], skills: vec![] })
            });
            prop_assert!(result.is_err());
        }
        #[test]
        fn secret_url_identity_never_enters_snapshot(secret in "[a-zA-Z0-9]{1,30}") {
            let query = format!("https://host.test/?key={secret}");
            let userinfo = format!("https://user:{secret}@host.test/");
            prop_assert!(public_url(&query).is_none());
            prop_assert!(public_url(&userinfo).is_none());
        }
    }
}
