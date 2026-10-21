//! 画师写出图提示词（owner-followups-1015 票 28，规格「画师」：「用其主对话模型 + 内置
//! 『出图提示词』技能为每个目标角色生成提示词」；2026-10-20 负责人裁决撤掉票 20 的固定模板）。
//!
//! 一次对话请求写全队：系统层 = 隐藏内置技能 `image-prompt` 的正文
//! （[`crate::presets::hidden_skill_doc`]），用户层 = JSON `{style, roles:[{role, duty_summary}]}`。
//! 一次而不是逐角色：模型同时看见全队才能给出同一套画风、又让各角色的主题互不重复。
//! 走与其他辅助调用（`draft_role_def`、开场分析）同一条路：`resolve_slot` 解析出的主对话
//! 供应商 → `model_failure::bounded_call` → `usage::request` 记账。被否决：在这里另起一个
//! HTTP 客户端直连供应商——绕开预算闸、请求账本与失败分类。
//!
//! 外发边界：输入只有风格、角色名、职责摘要（与出图请求同一组三样，规格用户故事 73），
//! 不带负责人原话、项目文件、对话；不给工具。用量记在画师名下（`agent_id` 空 → NULL，
//! `purpose` = [`PROMPT_PURPOSE`]，汇总读面归 `artist`）。
//!
//! 失败不回退模板（负责人裁决；规格只写了出图槽缺位的提示，没写主对话模型缺位）：
//! 没有模型 / 调用失败 / 输出不可用，一律由调用方发 [`PROMPT_FAILED`] 提示、一张图都不出。
//! 代价模型：误判「不可用」多一次重新召唤；误判「可用」= 把不是画师写的、或缺角色的
//! 提示词外发给出图服务商。偏向判不可用——缺任一角色、空提示词、超长都整单拒。

use serde_json::{json, Value};

use crate::db::Db;
use crate::image_gen::PromptModel;
use crate::provider::{ChatRequest, ContentBlock, Message, ProviderError, Role};
use crate::tools::ToolContext;

/// 写提示词这次对话请求在用量账上的用途（汇总读面归画师）。
pub const PROMPT_PURPOSE: &str = "artist_prompt";
/// 写不出提示词时画师那一句的 System 子类（登记于 `trace::SYSTEM_SUBKINDS`）。
pub const PROMPT_FAILED: &str = "artist_prompt_failed";
/// 画师的内置技能名（`presets/hidden_skills/image-prompt`）。
pub const SKILL: &str = "image-prompt";
/// 单条提示词上限（字符）。技能要求 600 以内；留余量，超出视为输出不可用
/// （dall-e-2 的硬上限是 1000）。
const MAX_PROMPT_CHARS: usize = 1000;

/// 交给主对话模型的一格：角色名 + 职责摘要（`image_gen::duty_summary`）。
pub struct RoleBrief {
    pub role: String,
    pub duty_summary: String,
}

/// 失败分类码（进 [`PROMPT_FAILED`] 载荷的 `reason`，界面按码取文案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptFailure {
    /// 画师的主对话槽与 default 都没有模型。
    ChatModelMissing,
    /// 已知用量上限触顶。
    BudgetExhausted,
    /// 供应商调用失败（重试窗口内仍失败）。
    ChatModelFailed,
    /// 回复不是约定的 JSON，或缺角色 / 空 / 超长。
    InvalidPrompts,
}

impl PromptFailure {
    pub fn code(self) -> &'static str {
        match self {
            Self::ChatModelMissing => "chat_model_missing",
            Self::BudgetExhausted => "budget_exhausted",
            Self::ChatModelFailed => "chat_model_failed",
            Self::InvalidPrompts => "invalid_prompts",
        }
    }
}

/// 为 `briefs` 里每个角色写一条提示词，按 `briefs` 顺序返回 `(角色, 提示词)`。
pub fn write(
    db: &Db,
    ctx: &ToolContext,
    model: Option<&PromptModel>,
    style: &str,
    briefs: &[RoleBrief],
) -> Result<Vec<(String, String)>, PromptFailure> {
    let started = std::time::Instant::now();
    let Some(model) = model else {
        note(ctx, started, true, PromptFailure::ChatModelMissing.code());
        return Err(PromptFailure::ChatModelMissing);
    };
    if model.fell_back {
        crate::diag::slot_fallback(
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            "artist_prompt",
            &model.slot,
            started,
        );
    }
    let instructions = crate::skills::hidden_skill_instructions(SKILL)
        .expect("hidden image-prompt skill is embedded");
    let input = json!({
        "style": style,
        "roles": briefs.iter().map(|b| json!({"role": b.role, "duty_summary": b.duty_summary}))
            .collect::<Vec<_>>(),
    });
    let req = ChatRequest {
        model_slot: model.slot.clone(),
        messages: vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text { text: instructions }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: input.to_string(),
                }],
            },
        ],
        tools: vec![],
    };
    // 画师不是 agents 表成员：空 agent_id 让账本行落 NULL（同项目级请求），
    // purpose 把它归到画师名下。
    let mut artist_ctx = ctx.clone();
    artist_ctx.agent_id = String::new();
    let provider = model.provider.as_ref();
    let resp = crate::model_failure::bounded_call(db, &artist_ctx, || {
        crate::usage::request(
            db,
            &artist_ctx,
            &req.model_slot,
            PROMPT_PURPOSE,
            provider,
            Some(&req),
            || provider.complete(&req),
        )
    });
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let f = if matches!(e, ProviderError::BudgetUnavailable) {
                PromptFailure::BudgetExhausted
            } else {
                PromptFailure::ChatModelFailed
            };
            note(ctx, started, true, f.code());
            return Err(f);
        }
    };
    let text = crate::intake::response_text(&resp);
    match parse(&text, briefs) {
        Some(out) => {
            note(ctx, started, false, "prompts_written");
            Ok(out)
        }
        None => {
            note(ctx, started, true, PromptFailure::InvalidPrompts.code());
            Err(PromptFailure::InvalidPrompts)
        }
    }
}

/// 解析 `{"prompts":[{"role","prompt"}]}`（容忍代码围栏与前后空白）。每个目标角色恰取
/// 第一条同名记录；任何一个缺失 / 空 / 超长 → None。多出的角色忽略（不外发）。
fn parse(text: &str, briefs: &[RoleBrief]) -> Option<Vec<(String, String)>> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let v: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    let items = v["prompts"].as_array()?;
    briefs
        .iter()
        .map(|b| {
            let prompt = items
                .iter()
                .find(|it| it["role"].as_str() == Some(b.role.as_str()))?["prompt"]
                .as_str()?
                .trim();
            (!prompt.is_empty() && prompt.chars().count() <= MAX_PROMPT_CHARS)
                .then(|| (b.role.clone(), prompt.to_string()))
        })
        .collect()
}

fn note(ctx: &ToolContext, started: std::time::Instant, warn: bool, code: &str) {
    crate::diag::note(
        crate::diag::CLASS_SLOT,
        warn,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "artist_prompt",
        code,
        started,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn briefs(roles: &[&str]) -> Vec<RoleBrief> {
        roles
            .iter()
            .map(|r| RoleBrief {
                role: r.to_string(),
                duty_summary: String::new(),
            })
            .collect()
    }

    #[test]
    fn parse_takes_each_role_once_in_brief_order_and_ignores_extras() {
        let text = "```json\n{\"prompts\":[{\"role\":\"B\",\"prompt\":\" b1 \"},{\"role\":\"X\",\"prompt\":\"x\"},{\"role\":\"A\",\"prompt\":\"a\"},{\"role\":\"B\",\"prompt\":\"b2\"}]}\n```";
        assert_eq!(
            parse(text, &briefs(&["A", "B"])),
            Some(vec![("A".into(), "a".into()), ("B".into(), "b1".into())])
        );
    }

    #[test]
    fn parse_rejects_missing_empty_or_overlong_prompts() {
        let b = briefs(&["A", "B"]);
        assert_eq!(parse("not json", &b), None);
        assert_eq!(
            parse(r#"{"prompts":[{"role":"A","prompt":"a"}]}"#, &b),
            None
        );
        assert_eq!(
            parse(
                r#"{"prompts":[{"role":"A","prompt":"a"},{"role":"B","prompt":"  "}]}"#,
                &b
            ),
            None
        );
        let long = "x".repeat(MAX_PROMPT_CHARS + 1);
        let text = json!({"prompts":[{"role":"A","prompt":"a"},{"role":"B","prompt":long}]});
        assert_eq!(parse(&text.to_string(), &b), None);
    }

    #[test]
    fn the_hidden_skill_is_embedded_and_not_in_the_loader() {
        let body = crate::skills::hidden_skill_instructions(SKILL).unwrap();
        assert!(body.contains("\"prompts\""), "技能约定输出格式");
        assert!(!body.starts_with("---"), "frontmatter 不进正文");
        assert!(
            !crate::presets::preset_skill_names().contains(&SKILL),
            "不进技能加载器的内置层（各角色目录看不到）"
        );
        let artist = crate::presets::hidden_roles().unwrap();
        assert_eq!(artist[0].role.skills, vec![SKILL.to_string()]);
    }
}
