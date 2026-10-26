//! 彩蛋「画师」（owner-followups-1015 票 19，`CONTEXT.md`「画师」，2026-10-17 负责人裁决）。
//!
//! 负责人在聊天里要求给团队设头像时，项目经理（调度方）按意图调用内部工具
//! [`SummonArtist`]，隐藏角色「画师」出场。**没有关键词匹配**：识别意图是模型的事，
//! 任何界面语言、任何说法都走同一个工具。被否决：宿主在负责人原话里找「头像」
//! 「avatar」——漏掉其他语言与说法，且会把「别改头像」也当成召唤。
//!
//! 画师不是 agents 表里的成员：不进花名册、@ 建议、侧栏团队（团队读面全读 agents 表，
//! 结构上就看不到它）。它在时间线上的发言是一条 System 事件（`artist_*` 子类），
//! payload 带 `speaker`，界面以画师的名字与头像渲染。
//!
//! 本票只打通出图模型「未绑定 / 不支持」这条路：出图槽**精确**检查
//! （[`provider_config::image_generation_slot`]，不回退 default——default 是对话模型，
//! 回退 = 拿对话模型凑合出图）。代价模型：误报「没绑」多一次去设置；误报「已绑」
//! 负责人以为能出图、实际外发到对话模型。偏向报没绑。
//! 已绑且支持时（票 20）走出图执行器（`image_gen`）出头像提案（`artist_proposal`）；
//! 每个角色的提示词先由画师的主对话模型配内置「出图提示词」技能撰写（票 28，`artist_prompt`）。

use crate::db::Db;
use crate::provider_config::{self, ImageGenerationSlot};
use crate::tools::{RiskClass, Tool, ToolContext, ToolError};
use serde_json::{json, Value};

/// 画师的定名（隐藏预置角色 `presets/hidden_roles.json` 的 name）。
pub const ARTIST_ROLE: &str = "画师";
/// 画师的代码名。
pub const ARTIST_CODE: &str = "artist";
/// 出图模型缺位时画师那一句的 System 子类（登记于 `trace::SYSTEM_SUBKINDS`）。
pub const IMAGE_MODEL_MISSING: &str = "artist_image_model_missing";

/// 画师的主对话槽（隐藏预置角色的 `model_slot`；读不到按 `chat`）。票 28：写提示词用。
pub fn artist_chat_slot() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| {
        crate::presets::hidden_roles()
            .ok()
            .and_then(|roles| roles.into_iter().find(|r| r.code == ARTIST_CODE))
            .map(|r| r.role.model_slot.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "chat".into())
    })
}

pub struct SummonArtist;

impl Tool for SummonArtist {
    fn name(&self) -> &str {
        "summon_artist"
    }
    fn description(&self) -> &str {
        "Project manager only. Use when the owner asks, in any wording or language, to give the team's roles avatars or profile pictures (for example one pixel-art avatar per role). Summons the hidden Artist, who writes one consistent-style image prompt per role and generates avatar proposals with the image model. Pass the owner's style in their words; pass roles only when the owner named specific team roles. If no image model is bound the Artist tells the owner how to bind one. Do not use for other images, for a single role's avatar the owner set by hand, or when you are not the project manager; do not generate or describe avatars yourself."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","required":["style"],"properties":{
            "style":{"type":"string","minLength":1,"maxLength":500,"description":"The avatar style the owner asked for, in the owner's words."},
            "roles":{"type":"array","maxItems":32,"items":{"type":"string"},"description":"Optional subset of current team role names; omit for the whole team."}
        }})
    }
    /// 不碰仓库、不改头像（应用只在负责人点「全部应用」时发生）。会外发出图请求，
    /// 但外发面只有角色名 / 职责摘要 / 风格描述，且先写轨迹（`image_gen` 模块注释）；
    /// 召唤本身是负责人在聊天里要求的。仍记 Read：Write/Exec 档会把这次召唤变成
    /// 权限卡，等于让负责人为自己刚提的要求再批一次。
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let started = std::time::Instant::now();
        let note = |class: &str, warn: bool, code: &str| {
            crate::diag::note(
                class,
                warn,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "artist_summon",
                code,
                started,
            )
        };
        // 调度方的工具：别的角色召唤画师 = 绕过调度给全队换头像。
        if crate::permissions::agent_role(db, ctx).as_deref() != Some(crate::pm_route::PM_ROLE) {
            note(crate::diag::CLASS_REJECT, true, "not_project_manager");
            return Err(ToolError::Exec(
                "only the project manager can summon the artist".into(),
            ));
        }
        let style = input["style"].as_str().unwrap_or("").trim();
        if style.is_empty() {
            return Err(ToolError::Exec("style is required".into()));
        }
        let roles = target_roles(db, ctx, input).inspect_err(|_| {
            note(crate::diag::CLASS_REJECT, true, "unknown_target_role");
        })?;
        let doc = provider_config::load().map_err(|e| ToolError::Exec(e.to_string()))?;
        let (state, model) = match provider_config::image_generation_slot(&doc) {
            ImageGenerationSlot::Unconfigured => ("unconfigured", None),
            ImageGenerationSlot::Unsupported { model } => ("unsupported", Some(model)),
            ImageGenerationSlot::Configured { model } => {
                return draw_proposal(db, ctx, &doc, style, &roles, &model, started);
            }
        };
        note(crate::diag::CLASS_SLOT, true, state);
        artist_notice(
            db,
            ctx,
            json!({"kind":IMAGE_MODEL_MISSING,"speaker":ARTIST_ROLE,"speaker_code":ARTIST_CODE,
                "slot":provider_config::IMAGE_SLOT,"state":state,"model":model,
                "style":style,"roles":roles}),
        )?;
        Ok(json!({"state":state,"model":model,
            "note":"the Artist told the owner an image model must be bound in Settings; do not generate avatars another way"}))
    }
}

/// 画师在时间线上的一条系统通知（缺槽 / 端点不可用 / 提示词失败）。
fn artist_notice(db: &Db, ctx: &ToolContext, payload: Value) -> Result<(), ToolError> {
    db.append_event(
        &ctx.project_id,
        crate::trace::EventKind::System,
        payload,
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )
    .map_err(|e| ToolError::Exec(format!("artist notice not recorded: {e}")))?;
    Ok(())
}

/// 已绑且支持出图（票 20）：解析端点 → 逐角色出图 → 头像提案。端点不可发（非 OpenAI 兼容 /
/// 供应商停用 / 缺钥匙）按「不支持」回同一句提示——负责人要做的事一样：去设置改出图槽。
fn draw_proposal(
    db: &Db,
    ctx: &ToolContext,
    doc: &provider_config::ProviderDoc,
    style: &str,
    roles: &[String],
    model: &str,
    started: std::time::Instant,
) -> Result<Value, ToolError> {
    let endpoint = ctx
        .images
        .as_ref()
        .ok_or_else(|| ToolError::Exec("image generation is not wired in this context".into()))
        .map(|access| access.endpoint(doc))?;
    let endpoint = match endpoint {
        Ok(ep) => ep,
        Err(f) => {
            crate::diag::note(
                crate::diag::CLASS_SLOT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "artist_summon",
                f.code,
                started,
            );
            artist_notice(
                db,
                ctx,
                json!({"kind":IMAGE_MODEL_MISSING,"speaker":ARTIST_ROLE,"speaker_code":ARTIST_CODE,
                    "slot":provider_config::IMAGE_SLOT,"state":"unsupported","model":model,
                    "reason":f.code,"style":style,"roles":roles}),
            )?;
            return Ok(json!({"state":"unsupported","model":model,"reason":f.code,
                "note":"the Artist told the owner the image slot cannot be used; do not generate avatars another way"}));
        }
    };
    let prompts = match write_prompts(db, ctx, style, roles)? {
        Ok(p) => p,
        Err(reply) => return Ok(reply),
    };
    let cx = crate::artist_proposal::DrawCtx {
        db,
        root: &ctx.repo_root,
        project_id: &ctx.project_id,
        agent_id: Some(&ctx.agent_id),
        stage_run_id: ctx.stage_run_id.as_deref(),
    };
    let proposal = crate::artist_proposal::create(&cx, &endpoint, style, &prompts)
        .map_err(|e| ToolError::Exec(format!("avatar proposal failed: {e}")))?;
    let drawn = proposal.items.iter().filter(|i| i.image.is_some()).count();
    Ok(
        json!({"state":"configured","model":model,"proposal_id":proposal.id,
        "drawn":drawn,"failed":proposal.items.len() - drawn,
        "note":"the Artist posted an avatar proposal card in the timeline; only the owner applies it. Do not apply or describe the avatars yourself."}),
    )
}

/// 写出图提示词；写不出来时落通知并给出回给调度方的工具结果（内层 `Err`）。
fn write_prompts(
    db: &Db,
    ctx: &ToolContext,
    style: &str,
    roles: &[String],
) -> Result<Result<Vec<(String, String)>, Value>, ToolError> {
    // 票 28：提示词由画师的主对话模型配内置「出图提示词」技能撰写。写不出来就明说、
    // 一张图都不出——不退回票 20 的固定模板（2026-10-20 负责人裁决；模块注释见 artist_prompt）。
    let briefs: Vec<_> = roles
        .iter()
        .map(|role| crate::artist_prompt::RoleBrief {
            role: role.clone(),
            duty_summary: crate::artist_proposal::duty_summary_of(db, &ctx.project_id, role),
        })
        .collect();
    let model_ref = ctx.images.as_ref().and_then(|a| a.prompt_model());
    match crate::artist_prompt::write(db, ctx, model_ref, style, &briefs) {
        Ok(p) => Ok(Ok(p)),
        Err(f) => {
            artist_notice(
                db,
                ctx,
                json!({"kind":crate::artist_prompt::PROMPT_FAILED,"speaker":ARTIST_ROLE,
                    "speaker_code":ARTIST_CODE,"reason":f.code(),"slot":artist_chat_slot(),
                    "style":style,"roles":roles}),
            )?;
            Ok(Err(json!({"state":"prompt_failed","reason":f.code(),
                "note":"the Artist could not write the image prompts and told the owner; nothing was sent to the image model. Do not write prompts or generate avatars another way."})))
        }
    }
}

/// 目标角色集：缺省 = 全队；给了就必须都是当前团队成员（画师自己不在团队里，自然被拒）。
fn target_roles(db: &Db, ctx: &ToolContext, input: &Value) -> Result<Vec<String>, ToolError> {
    let team = crate::roles::team_roles(db, &ctx.project_id)
        .map_err(|e| ToolError::Exec(e.to_string()))?;
    let Some(asked) = input["roles"].as_array() else {
        return Ok(team);
    };
    let mut out = Vec::new();
    for role in asked {
        let role = role.as_str().unwrap_or("");
        if !team.iter().any(|r| r == role) {
            return Err(ToolError::Exec(format!("not a team role: {role}")));
        }
        if !out.iter().any(|r: &String| r == role) {
            out.push(role.to_string());
        }
    }
    Ok(out)
}
