//! 聊天触发登记与发布（owner-followups-1015 #16，ADR 0093 后续裁决 / CONTEXT.md「发布目标」「远程发布」）。
//!
//! 负责人在聊天里用自然语言要求登记或发布；Agent 只拟稿，两样都落到负责人亲手批准的卡上：
//! - [`ProposeReleaseTarget`]（`propose_release_target`）：拟登记发布目标 → 「登记发布目标」卡（kind
//!   `release_target`，迁移 0067）。**批准前不落库**：拟稿只在卡载荷里，[`approve`] 才经
//!   `release_targets::save` 写入；托管不代批（`api/managed/routes.rs::decision_key` 对本卡种返回 None，
//!   卡留给负责人；[`approve`] 自己也只认 `owner`）。
//! - [`RequestRelease`]（`request_release`）：发起发布，只接受已登记目标（id 或登记名精确相等），
//!   走票 15 的 `release::request` → 发布确认卡（必问、永不记忆），不另起一套卡与前置闸。
//!
//! **模型文本不构成授权**（ADR 0092 / 0093）：工具调用本身不授权任何东西——它只产卡；没有任何入参能
//! 让工具直接登记或执行。识别意图是模型的事（同 `artist::SummonArtist`：不做关键词匹配）。
//!
//! **聊天里疑似凭据不转述、不进轨迹**：拟稿任一字段疑似夹着凭据值（`release_secret::suspected`）即拒绝、
//! 不产卡，错误文案不回显字段；工具调用事件的入参同一判定脱敏（`tools::safety::scrub_input`）。
//! 批准时再用已存凭据值精确比对一次（沿用 `release_secret_in_target`）。
//!
//! 两个工具都记 `RiskClass::Read`：它们不碰仓库、不跑命令，唯一的副作用是给负责人出一张卡；
//! 记 Write/Exec 会先弹一张权限卡，等于让负责人为「给我出一张卡」再批一次。

use serde_json::{json, Value};

use crate::cards::{CardKind, CardState};
use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::release::ReleaseRunError;
use crate::release_targets::{self as targets, ReleaseError, ReleaseTarget, ReleaseTargetDraft};
use crate::tools::{RiskClass, Tool, ToolContext, ToolError};
use crate::trace::EventKind;

pub const PROPOSE_TOOL: &str = "propose_release_target";
pub const REQUEST_TOOL: &str = "request_release";
/// System 子类（登记于 `trace::SYSTEM_SUBKINDS`）。
pub const PROPOSED: &str = "release_target_proposed";
pub const PROPOSAL_ANSWERED: &str = "release_target_proposal_answered";
/// 疑似凭据被替换成的字样（工具调用事件里）。
pub const REDACTED: &str = "[redacted]";

fn note(branch: &str, warn: bool, agent: Option<&str>, qid: Option<&str>, code: &str) {
    crate::diag::note(
        if warn {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_HOST
        },
        warn,
        Some(crate::PROJECT_ID),
        agent,
        None,
        qid,
        branch,
        code,
        std::time::Instant::now(),
    );
}

fn fields(d: &ReleaseTargetDraft) -> Vec<&str> {
    [d.name.as_str(), d.command.as_str(), d.cwd.as_str()]
        .into_iter()
        .chain(d.credentials.iter().map(String::as_str))
        .collect()
}

fn draft_of(p: &Value) -> ReleaseTargetDraft {
    let text = |k: &str| p[k].as_str().unwrap_or_default().to_string();
    ReleaseTargetDraft {
        id: None,
        name: text("name"),
        command: text("command"),
        cwd: text("cwd"),
        credentials: p["credentials"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Agent 拟登记：判定 → 入队「登记发布目标」卡。不写 `release_targets`。
/// 同一拟稿已有待决卡时返回那张（模型重试不叠卡）。
pub fn propose(
    db: &Db,
    project_id: &str,
    agent_id: &str,
    raw: &ReleaseTargetDraft,
) -> Result<String, ReleaseRunError> {
    // 先判疑似凭据、再做形状校验：形状错误的文案会带上名称 / 工作目录原文。
    if crate::release_secret::any_suspected(fields(raw)) {
        note(
            "release_propose",
            true,
            Some(agent_id),
            None,
            "release_secret_suspected",
        );
        return Err(ReleaseRunError::ReleaseSecretSuspected);
    }
    let draft = ReleaseTargetDraft {
        id: None,
        name: targets::normalize_name(&raw.name)?,
        command: targets::normalize_command(&raw.command)?,
        cwd: targets::normalize_cwd(&raw.cwd)?,
        credentials: targets::normalize_credentials(&raw.credentials)?,
    };
    if targets::names(db, project_id)?.contains(&draft.name) {
        return Err(ReleaseError::ReleaseNameTaken(draft.name).into());
    }
    let payload = json!({
        "name": draft.name,
        "command": draft.command,
        "cwd": draft.cwd,
        "credentials": draft.credentials,
        "proposed_by": agent_id,
    });
    if let Some(open) = crate::cards::queued(db, project_id)?.into_iter().find(|c| {
        c.kind == CardKind::ReleaseTarget.as_str()
            && ["name", "command", "cwd", "credentials"]
                .iter()
                .all(|k| c.payload[*k] == payload[*k])
    }) {
        return Ok(open.id);
    }
    let qid = crate::cards::enqueue(
        db,
        project_id,
        Some(agent_id),
        CardKind::ReleaseTarget,
        payload,
        None,
    )?;
    // 轨迹只记名字（同 release_target_changed）：命令行原文在卡面，批准后在登记表。
    db.append_event(
        project_id,
        EventKind::System,
        json!({"kind": PROPOSED, "question_id": qid, "name": draft.name,
               "credentials": draft.credentials, "by": agent_id}),
        Some(agent_id),
        None,
    )?;
    note(
        "release_propose",
        false,
        Some(agent_id),
        Some(&qid),
        "proposed",
    );
    Ok(qid)
}

fn card(db: &Db, project_id: &str, qid: &str) -> Result<crate::cards::Card, ReleaseRunError> {
    let card = crate::cards::get(db, qid)
        .ok()
        .filter(|c| c.kind == CardKind::ReleaseTarget.as_str() && c.project_id == project_id)
        .ok_or_else(|| ReleaseRunError::ReleaseProposalUnknown(qid.into()))?;
    if card.state != CardState::Queued {
        return Err(ReleaseRunError::ReleaseProposalAnswered(qid.into()));
    }
    Ok(card)
}

fn answered(
    db: &Db,
    project_id: &str,
    qid: &str,
    name: &str,
    by: &str,
    extra: Value,
) -> Result<(), ReleaseRunError> {
    let mut payload =
        json!({"kind": PROPOSAL_ANSWERED, "question_id": qid, "name": name, "by": by});
    if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    db.append_event(project_id, EventKind::System, payload, None, None)?;
    Ok(())
}

/// 负责人批准拟稿：守门 → 认领 → 登记。任何守门不成立都不认领，卡留在待决里（负责人可驳回）。
pub fn approve(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
    by: &str,
) -> Result<ReleaseTarget, ReleaseRunError> {
    let card = card(db, project_id, qid)?;
    // ADR 0093 后续裁决「托管不得新登记」：裁决人不是负责人（`managed:<grant>:<gen>` 等）一律拒绝。
    // 今天托管路径到不了这里（decision_key 不接管本卡种）；这一道是以后加路径时的默认拒绝。
    // 代价模型：误拒只让非负责人的调用报错；漏放是托管登记了一条它随后可以代批执行的命令。
    if by != "owner" {
        note(
            "release_proposal_approve",
            true,
            None,
            Some(qid),
            "release_owner_only",
        );
        return Err(ReleaseError::ReleaseOwnerOnly.into());
    }
    let draft = draft_of(&card.payload);
    let visible = fields(&draft);
    // 卡载荷入队前已判过；这里再判一次，免得旧卡 / 手改的载荷绕过。
    if crate::release_secret::any_suspected(visible.iter().copied()) {
        note(
            "release_proposal_approve",
            true,
            None,
            Some(qid),
            "release_secret_suspected",
        );
        return Err(ReleaseRunError::ReleaseSecretSuspected);
    }
    // 沿用 release_secret_in_target：已存的任一发布凭据值出现在拟稿里就拒绝（Agent 从聊天里抄到了值）。
    let stored = targets::stored_secrets(db, project_id, store)?;
    if stored
        .iter()
        .any(|s| visible.iter().any(|f| f.contains(s.as_str())))
    {
        note(
            "release_proposal_approve",
            true,
            None,
            Some(qid),
            "release_secret_in_target",
        );
        return Err(ReleaseError::ReleaseSecretInTarget.into());
    }
    drop(stored);
    if targets::names(db, project_id)?.contains(&draft.name) {
        return Err(ReleaseError::ReleaseNameTaken(draft.name).into());
    }
    if !crate::cards::claim(db, project_id, qid, CardKind::ReleaseTarget, by)? {
        return Err(ReleaseRunError::ReleaseProposalAnswered(qid.into()));
    }
    match targets::save(db, project_id, store, &draft) {
        Ok(target) => {
            answered(
                db,
                project_id,
                qid,
                &target.name,
                by,
                json!({"decision": "approved", "target_id": target.id}),
            )?;
            note(
                "release_proposal_approve",
                false,
                None,
                Some(qid),
                "registered",
            );
            Ok(target)
        }
        Err(e) => {
            let code = crate::errcode::variant_code(&e);
            answered(
                db,
                project_id,
                qid,
                &draft.name,
                by,
                json!({"decision": "failed", "error": code}),
            )?;
            note("release_proposal_approve", true, None, Some(qid), &code);
            Err(e.into())
        }
    }
}

/// 驳回拟稿：认领 + 轨迹，什么都不登记。
pub fn reject(db: &Db, project_id: &str, qid: &str, by: &str) -> Result<(), ReleaseRunError> {
    let card = card(db, project_id, qid)?;
    if !crate::cards::claim(db, project_id, qid, CardKind::ReleaseTarget, by)? {
        return Err(ReleaseRunError::ReleaseProposalAnswered(qid.into()));
    }
    let name = card.payload["name"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    answered(
        db,
        project_id,
        qid,
        &name,
        by,
        json!({"decision": "rejected"}),
    )
}

/// 工具调用事件的入参脱敏：疑似凭据的字符串（含数组里的）换成 [`REDACTED`]，其余原样。
pub(crate) fn scrub(input: &Value) -> Value {
    match input {
        Value::String(s) if crate::release_secret::suspected(s) => json!(REDACTED),
        Value::Array(a) => a.iter().map(scrub).collect(),
        Value::Object(m) => m.iter().map(|(k, v)| (k.clone(), scrub(v))).collect(),
        other => other.clone(),
    }
}

/// 回合观测（`turn::persist_activation_observation`，模型原样回复入轨迹）里本模块两个工具的入参同样脱敏。
/// 这条曾经漏过：只脱敏工具调用事件时，疑似凭据仍随 `activation_observation` 的 tool_use 块进了轨迹
/// （票 16 回归测试 `a_suspected_credential_never_reaches_a_card_or_the_trace` 抓到）。只动这两个工具：
/// 其他工具的观测要原样留给恢复回合重建上下文。恢复时这两个工具看到的是脱敏后的入参，
/// 重放会撞上 `tool_actions` 的入参比对（action identity reused）而不执行，不会补出一张卡。
pub(crate) fn scrub_observation(
    content: &[crate::provider::ContentBlock],
) -> Vec<crate::provider::ContentBlock> {
    use crate::provider::ContentBlock;
    content
        .iter()
        .map(|block| match block {
            ContentBlock::ToolUse { id, name, input }
                if name == PROPOSE_TOOL || name == REQUEST_TOOL =>
            {
                ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: scrub(input),
                }
            }
            other => other.clone(),
        })
        .collect()
}

fn tool_error(e: ReleaseRunError) -> ToolError {
    ToolError::Exec(e.to_string())
}

pub struct ProposeReleaseTarget;

impl Tool for ProposeReleaseTarget {
    fn name(&self) -> &str {
        PROPOSE_TOOL
    }
    fn description(&self) -> &str {
        "Use when the owner asks in chat, in any wording or language, to register or remember a release command (for example \"record npm publish as our release command\"). Drafts a release target onto a Register release target card: a short name, the single-line command, the working directory relative to the project root (empty for the root), and the names of the credentials the command reads as environment variables (like NPM_TOKEN). Nothing is registered until the owner approves the card. Never put a credential value in any field and never repeat a value the owner pasted in chat: list only the credential's name and tell the owner to enter the value in Settings · Release. Do not use to change or delete an existing target; the owner does that in Settings · Release."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","additionalProperties":false,"required":["name","command"],"properties":{
            "name":{"type":"string","minLength":1,"maxLength":64,"description":"Short release target name, e.g. npm."},
            "command":{"type":"string","minLength":1,"maxLength":2000,"description":"Single-line command, credentials referenced as environment variables (e.g. $NPM_TOKEN), never their values."},
            "cwd":{"type":"string","maxLength":500,"description":"Working directory relative to the project root; empty for the root."},
            "credentials":{"type":"array","maxItems":16,"items":{"type":"string","maxLength":64},"description":"Credential names (environment-variable style, e.g. NPM_TOKEN). Names only."}
        }})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let qid =
            propose(db, &ctx.project_id, &ctx.agent_id, &draft_of(input)).map_err(tool_error)?;
        Ok(json!({
            "question_id": qid,
            "status": "awaiting_owner_approval",
            "registered": false,
            "next": "The owner approves or declines the Register release target card. Credential values are entered by the owner in Settings · Release.",
        }))
    }
}

pub struct RequestRelease;

impl Tool for RequestRelease {
    fn name(&self) -> &str {
        REQUEST_TOOL
    }
    fn description(&self) -> &str {
        "Use when the owner asks in chat, in any wording or language, to release, publish or ship a version. Only registered release targets can be released: pass the target's exact registered name. This puts a release confirmation card in front of the owner showing the registered command; nothing runs until it is confirmed there. If no registered target fits, tell the owner which targets exist. Do not use to register or change a target (use propose_release_target), and never run release commands yourself with shell tools or invent a command."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","additionalProperties":false,"required":["target"],"properties":{
            "target":{"type":"string","minLength":1,"maxLength":64,"description":"Exact registered release target name."}
        }})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let key = input["target"].as_str().unwrap_or_default();
        let found = targets::find(db, &ctx.project_id, key).map_err(|e| tool_error(e.into()))?;
        let Some(target) = found else {
            // 不回显入参（可能是模型塞进来的命令或令牌），只列能选的名字。
            note(
                "release_request",
                true,
                Some(&ctx.agent_id),
                None,
                "release_target_unregistered",
            );
            let names = targets::names(db, &ctx.project_id).map_err(|e| tool_error(e.into()))?;
            return Err(ToolError::Exec(if names.is_empty() {
                "no release target is registered; the owner registers one in Settings · Release (or approve a propose_release_target card)".into()
            } else {
                format!(
                    "not a registered release target; registered: {}",
                    names.join(", ")
                )
            }));
        };
        let qid = crate::release::request(db, &ctx.project_id, &target.id, &ctx.agent_id)
            .map_err(tool_error)?;
        Ok(json!({
            "question_id": qid,
            "target": target.name,
            "status": "awaiting_owner_confirmation",
            "executed": false,
        }))
    }
}
