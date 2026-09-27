//! D03/D08: real transports share one paid authority across hosts and activities.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LivePrice {
    pub model: String,
    pub provider_id: String,
    pub tariff: DebugPrice,
    pub source_fingerprint: String,
}

pub(super) fn authority(binding: &Binding) -> io::Result<Db> {
    match binding.scope.as_str() {
        DEBUG => Db::open_current(Path::new(&binding.host).join(".hexagon/state.db")).map_err(err),
        PAID => Db::open_current(paid_path()?).map_err(err),
        _ => Err(rejected("unknown_budget_scope")),
    }
}

fn paid_db() -> io::Result<Db> {
    let path = paid_path()?;
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| rejected("authority_parent_missing"))?,
    )?;
    let db = Db::open(path).map_err(err)?;
    db.conn().execute("INSERT OR IGNORE INTO evaluation_budget_rounds(id,total_mc,pilot_mc) VALUES (?1,?2,?3)",params![PAID,sql(TOTAL)?,sql(PILOT)?]).map_err(err)?;
    Ok(db)
}

pub(crate) fn reserve_plan(
    host: &Db,
    root: &Path,
    plan_id: &str,
    price_json: &str,
) -> io::Result<()> {
    let price: LivePrice = serde_json::from_str(price_json)?;
    bound(&price.tariff)?;
    let plan = plan::read(host, plan_id)?;
    let next = plan
        .entries
        .iter()
        .find(|r| r.state == plan::PlannedState::Planned)
        .ok_or_else(|| rejected("plan_exhausted"))?;
    let batch = config::read(host, &plan.batch_id)?;
    let db = paid_db()?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    reserve_pair(
        &db,
        PAID,
        root,
        &plan,
        next,
        &batch.request.limits,
        price_json,
    )?;
    tx.commit().map_err(err)
}

pub(crate) fn bind_plan(host: &Db, root: &Path, workspace: &Path, run: &str) -> io::Result<()> {
    let (plan, position): (String, i64) = host
        .conn()
        .query_row(
            "SELECT plan_id,position FROM evaluation_plan_runs WHERE run_id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(err)?;
    let db = paid_db()?;
    let host_id = canonical(root)?;
    let key:String=db.conn().query_row("SELECT r.id FROM evaluation_budget_runs r JOIN evaluation_budget_pairs p ON r.pair_id=p.id WHERE p.host=?1 AND p.plan_id=?2 AND r.position=?3",params![host_id,plan,position],|r|r.get(0)).map_err(err)?;
    let changed=db.conn().execute("UPDATE evaluation_budget_runs SET run_id=?2,workspace=?3 WHERE id=?1 AND run_id IS NULL AND closed=0",params![key,run,canonical(workspace)?]).map_err(err)?;
    if changed != 1 {
        return Err(rejected("paid_worker_already_bound"));
    }
    std::fs::create_dir_all(workspace.join(".hexagon"))?;
    std::fs::write(
        workspace.join(".hexagon/evaluation-budget.json"),
        serde_json::to_vec(&Binding {
            host: host_id,
            run_key: key,
            scope: PAID.into(),
        })?,
    )?;
    Ok(())
}

pub(crate) fn bind_activity(
    root: &Path,
    workspace: &Path,
    operation: &str,
    limits: &config::EvaluationLimits,
    price: &LivePrice,
    pilot: bool,
    requests: u32,
) -> io::Result<()> {
    bound(&price.tariff)?;
    if requests == 0 || requests > limits.requests {
        return Err(rejected("activity_request_limit_invalid"));
    }
    let db = paid_db()?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let summary = summary(&db, PAID)?;
    if summary.blocked
        || !fits(
            summary.known_mc,
            summary.reserved_mc,
            limits.run_mc,
            summary.limit_mc.min(limits.total_mc),
        )
        || (pilot
            && !fits(
                summary.pilot_exposure_mc,
                0,
                limits.run_mc,
                summary.pilot_limit_mc.min(limits.pilot_mc),
            ))
    {
        return Err(rejected("activity_budget_unavailable"));
    }
    let host = canonical(root)?;
    let key = config::digest(&(PAID, &host, operation))?;
    tx.execute("INSERT INTO evaluation_budget_pairs(id,round_id,host,plan_id,pilot,allowance_mc,expected_runs) VALUES (?1,?2,?3,?4,?5,?6,1)",params![key,PAID,host,operation,pilot,sql(limits.run_mc)?]).map_err(err)?;
    tx.execute("INSERT INTO evaluation_budget_runs(id,pair_id,position,run_id,workspace,allowance_mc,request_limit,price_json) VALUES (?1,?1,0,?2,?3,?4,?5,?6)",params![key,operation,canonical(workspace)?,sql(limits.run_mc)?,sql(requests)?,serde_json::to_string(price)?]).map_err(err)?;
    tx.commit().map_err(err)?;
    std::fs::write(
        workspace.join(".hexagon/evaluation-budget.json"),
        serde_json::to_vec(&Binding {
            host,
            run_key: key,
            scope: PAID.into(),
        })?,
    )?;
    Ok(())
}

pub(crate) fn finish_activity(root: &Path, workspace: &Path, operation: &str) -> io::Result<()> {
    let db = paid_db()?;
    db.conn().execute("UPDATE evaluation_budget_runs SET closed=1 WHERE run_id=?1 AND workspace=?2 AND pair_id IN (SELECT id FROM evaluation_budget_pairs WHERE host=?3)",params![operation,canonical(workspace)?,canonical(root)?]).map_err(err)?;
    Ok(())
}

pub(crate) fn validate_request(
    provider: &dyn ModelProvider,
    chat: Option<&crate::provider::ChatRequest>,
    price: &LivePrice,
) -> io::Result<()> {
    let chat = chat.ok_or_else(|| rejected("paid_request_envelope_missing"))?;
    if provider.is_scripted()
        || provider.billing_model() != Some(price.model.as_str())
        || provider
            .output_token_limit()
            .is_none_or(|n| n > price.tariff.output_bound)
        || !provider.server_tools(&chat.model_slot).is_empty()
    {
        return Err(rejected("paid_model_or_bound_mismatch"));
    }
    use crate::provider::ContentBlock;
    if chat.messages.iter().flat_map(|m| &m.content).any(|b| {
        matches!(b, ContentBlock::Image { .. } | ContentBlock::Opaque { .. })
            || matches!(b,ContentBlock::ToolResult{images,..} if !images.is_empty())
    }) {
        return Err(rejected("paid_billing_dimension_unsupported"));
    }
    // UTF-8 JSON bytes + protocol margin bound textual inputs conservatively;
    // a tokenizer estimate alone can undercount a different supplier tokenizer.
    let bytes = serde_json::to_vec(&(&chat.messages, &chat.tools))?.len() as u64;
    if bytes
        .checked_add(4096)
        .is_none_or(|n| n > price.tariff.prompt_bound)
    {
        return Err(rejected("paid_prompt_bound_exceeded"));
    }
    Ok(())
}

/// D10/17: recover by host activity identity, even if death occurred between
/// authority commit and writing the worker binding. No request is reissued.
pub(crate) fn recover_activity(
    host: &Db,
    root: &Path,
    workspace: &Path,
    operation: &str,
    paid: bool,
) -> io::Result<()> {
    let authority = if paid { Some(paid_db()?) } else { None };
    let db = authority.as_ref().unwrap_or(host);
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let keys = {
        let mut q=tx.prepare("SELECT r.id FROM evaluation_budget_runs r JOIN evaluation_budget_pairs p ON p.id=r.pair_id WHERE p.host=?1 AND p.plan_id=?2 AND r.run_id=?2 AND r.workspace=?3").map_err(err)?;
        let rows = q
            .query_map(
                params![canonical(root)?, operation, canonical(workspace)?],
                |r| r.get::<_, String>(0),
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        rows
    };
    if keys.len() > 1 {
        return Err(rejected("activity_budget_ambiguous"));
    }
    for key in keys {
        tx.execute("UPDATE evaluation_budget_requests SET state='unknown',held_mc=MAX(bound_mc,held_mc) WHERE run_key=?1 AND state='pending'",[&key]).map_err(err)?;
        tx.execute("UPDATE evaluation_budget_rounds SET blocked=1 WHERE id=?1 AND EXISTS(SELECT 1 FROM evaluation_budget_requests WHERE run_key=?2 AND state='unknown')",params![if paid {PAID} else {DEBUG},key]).map_err(err)?;
        tx.execute(
            "UPDATE evaluation_budget_runs SET closed=1 WHERE id=?1",
            [&key],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatRequest, ChatResponse, ContentBlock, Message, ProviderError, Role};
    use proptest::prelude::*;
    struct Transport {
        scripted: bool,
        model: String,
        output: Option<u64>,
        hidden: bool,
    }
    impl ModelProvider for Transport {
        fn is_scripted(&self) -> bool {
            self.scripted
        }
        fn billing_model(&self) -> Option<&str> {
            Some(&self.model)
        }
        fn output_token_limit(&self) -> Option<u64> {
            self.output
        }
        fn server_tools(&self, _: &str) -> Vec<serde_json::Value> {
            if self.hidden {
                vec![serde_json::json!({"type":"web_search"})]
            } else {
                vec![]
            }
        }
        fn complete(&self, _: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            unreachable!("admission properties dispatch no request")
        }
    }
    proptest! {
        #[test]
        fn paid_admission_requires_exact_model_and_supported_bounded_envelope(text in "[a-z]{0,120}", limit in 1u64..10000, mutation in 0u8..7) {
            let mut provider=Transport{scripted:false,model:"model".into(),output:Some(limit),hidden:false};
            let mut request=ChatRequest{model_slot:"default".into(),messages:vec![Message{role:Role::User,content:vec![ContentBlock::Text{text}]}],tools:vec![]};
            let exact=serde_json::to_vec(&(&request.messages,&request.tools)).unwrap().len() as u64+4096;
            let mut price=LivePrice{model:"model".into(),provider_id:"provider".into(),source_fingerprint:"price".into(),tariff:DebugPrice{prompt_per_1k_mc:1,completion_per_1k_mc:1,prompt_bound:exact,output_bound:limit}};
            prop_assert!(validate_request(&provider,Some(&request),&price).is_ok());
            prop_assert!(validate_request(&provider,None,&price).is_err());
            match mutation {
                0=>provider.scripted=true,
                1=>provider.model.push('x'),
                2=>provider.output=None,
                3=>provider.output=Some(limit+1),
                4=>provider.hidden=true,
                5=>price.tariff.prompt_bound=exact-1,
                _=>request.messages[0].content.push(ContentBlock::Opaque{raw:serde_json::json!({})}),
            }
            prop_assert!(validate_request(&provider,Some(&request),&price).is_err());
        }
    }
}
