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
//! 已绑且支持时的出图执行器与头像提案卡是票 20。

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
    /// 只读出图槽配置、写一条时间线事件；不碰仓库、不外发（出图外发在票 20）。
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
                note(crate::diag::CLASS_SLOT, false, "image_executor_pending");
                return Ok(json!({"state":"configured","model":model,
                    "note":"image generation is not available in this build yet; tell the owner avatars cannot be generated yet"}));
            }
        };
        note(crate::diag::CLASS_SLOT, true, state);
        db.append_event(
            &ctx.project_id,
            crate::trace::EventKind::System,
            json!({"kind":IMAGE_MODEL_MISSING,"speaker":ARTIST_ROLE,"speaker_code":ARTIST_CODE,
                "slot":provider_config::IMAGE_SLOT,"state":state,"model":model,
                "style":style,"roles":roles}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )
        .map_err(|e| ToolError::Exec(format!("artist notice not recorded: {e}")))?;
        Ok(json!({"state":state,"model":model,
            "note":"the Artist told the owner an image model must be bound in Settings; do not generate avatars another way"}))
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
