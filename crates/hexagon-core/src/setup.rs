//! 项目向导（票 24，脏树/重开见 ADR 0060）：目录检查 → 角色/包选择 → 说明文件 → 密钥 → 建项目。
//!
//! - 目录：`inspect_dir` 报 git/脏/空/已有工作台/说明文件。脏树可开（不提交、不清理）；
//!   无 git 须确认初始化；已有 `.hexagon/state.db` 拒绝再建，走 `open_existing`。
//! - 说明文件主读 `AGENTS.md`，没有则 `CLAUDE.md`。空目录的一句话经
//!   `optimize_agents_md`（ADR 0067，主对话模型）填草稿；人确认后才
//!   `write_agents_md`，已有任一份说明都不覆盖。非空目录第一次成为项目时
//!   把 `opening_intake` 记成 pending，进工作台后才分析（票 17）；空目录记 skip。
//! - 密钥 fail-closed：所选角色的模型槽缺 key 不能开跑（`create_project` 内置复查）。
//! - 建项目把 RoleDef 落成实例：model_slot、agent_globs 归属、grants 技能授权。
//! - 票 14：最后一步按 `CREATE_STEP_ORDER` 逐步报告。每步做完才回调，失败不回调
//!   该步及之后的步，调用方因此不能把「打开项目」当成已成功。不用定时器假装进度。

use crate::api::Workbench;
use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::git;
use crate::orchestra::PackDef;
use crate::presets::{preset_roles, PresetError, RoleDef};
use crate::provider::{ChatRequest, ContentBlock, Message, ModelProvider, Role};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error(transparent)]
    Api(#[from] crate::api::ApiError),
    #[error(transparent)]
    Preset(#[from] PresetError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Cred(#[from] crate::credentials::CredError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("git: {0}")]
    Git(#[from] git::GitError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("目录不是 git 仓库，需确认初始化: {0}")]
    NoGit(PathBuf),
    #[error("目录已有工作台状态，请打开而不是新建: {0}")]
    AlreadyProject(PathBuf),
    #[error("目录没有工作台状态，不能按已有项目打开: {0}")]
    NotAProject(PathBuf),
    #[error("未知预置角色: {0}")]
    UnknownRole(String),
    #[error("角色定制传了未勾选的角色: {0}")]
    StrayOverride(String),
    #[error("角色定制不能自审: {0}")]
    SelfReviewer(String),
    #[error("角色定制「{role}」的上级「{reviewer}」不在模板目录内")]
    UnknownOverrideReviewer { role: String, reviewer: String },
    #[error("缺模型密钥，补齐或拿掉对应角色才能开跑: {}", .0.join(", "))]
    MissingKeys(Vec<String>),
    #[error("说明文件已存在，不覆盖: {0}")]
    AgentsMdExists(PathBuf),
    #[error("快速通道须指定一个已勾选角色")]
    NoFastRole,
    #[error(transparent)]
    Autonomy(#[from] crate::autonomy::AutonomyError),
    #[error("一句话是空的，不能优化项目说明")]
    EmptyBrief,
    #[error("主对话模型没有返回项目说明正文")]
    EmptyDraft,
    #[error("流程草稿不是一份流程包: {0}")]
    BadFlow(String),
    #[error("答问草稿不是 JSON 数组: {0}")]
    BadQuestions(String),
    #[error("角色起草不是 JSON 数组: {0}")]
    BadRoleDefs(String),
    #[error(transparent)]
    Model(#[from] crate::provider::ProviderError),
}

/// 目录体检报告：向导每一步的判定依据。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DirReport {
    pub exists: bool,
    /// 目录存在且没有条目（.hexagon 等隐藏项也算条目）
    pub empty: bool,
    pub is_git: bool,
    pub dirty: bool,
    /// 已有工作台状态（`.hexagon/state.db`）。向导走打开，`create_project` 拒绝再建。
    pub has_workbench: bool,
    /// 主说明文件：AGENTS.md 优先，其次 CLAUDE.md；都没有 = None
    pub instructions: Option<String>,
}

pub fn inspect_dir(dir: impl AsRef<Path>) -> DirReport {
    let dir = dir.as_ref();
    let exists = dir.is_dir();
    let empty = exists
        && std::fs::read_dir(dir)
            .map(|mut it| it.next().is_none())
            .unwrap_or(true);
    let is_git = exists && git::is_repo(dir);
    let has_workbench = exists && dir.join(".hexagon/state.db").is_file();
    let instructions = ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .find(|f| dir.join(f).is_file())
        .map(|f| f.to_string());
    DirReport {
        exists,
        empty,
        is_git,
        dirty: is_git && git::is_dirty(dir),
        has_workbench,
        instructions,
    }
}

/// 无模型时的结构骨架。票 16 之后空目录走 `optimize_agents_md`，
/// 不再用这份骨架冒充优化结果。非空目录的旧勾选路径仍可用。
pub fn agents_md_draft(project_name: &str) -> String {
    format!(
        "# {project_name}\n\n## Commands\n\n- Build:\n- Test:\n- Check:\n\n## Layout\n\n-\n\n## Conventions\n\n-\n"
    )
}

/// ADR 0067 优化描述的系统提示词（ADR 0071 修订：英文，输出用界面语言）。
/// 占位 `{project_name}`、`{language}`。正文是合同，与 ADR 0067 原文逐字一致。
/// 2026-09-24 前的版本要求「用用户那句话的语言来写」；负责人裁决改为跟随
/// 界面语言（prompt-engineering spec Q21），界面切到哪种语言草稿就是哪种。
/// 2026-09-25 修订：骨架补 Tech stack 一节（owner 要「意向技术选型」进草稿），
/// 并声明一句话后可能跟答问块——先问后答的优化流把答案拼进用户消息。
pub const AGENTS_MD_OPTIMIZE_PROMPT: &str = "You are drafting AGENTS.md at the repository root. This file holds the project-level constraints that activated agents read. It is not a skill; do not write process progress, secrets or a role roster into it.

The user gave a single sentence, possibly followed by answers to clarifying questions. Fill in the skeleton below so the file is as complete as the input honestly allows. For commands, tech stack or directories the user did not mention, leave them empty or write \"{unknown}\" — never invent them. Write the prose in {language}; keep the headings exactly as given.

# {project_name}

## Purpose
Two or three sentences. What the project does, who it serves, and — if the user's sentence implies boundaries — what it does not do.

## Tech stack
Intended languages, frameworks and key tools, one per line. Only what the user stated or picked in the answers; write \"{unknown}\" when nothing was stated.

## Commands
- Build:
- Test:
- Check:

## Layout
-

## Conventions
-

Output only the body of this file, with no preamble.";

fn optimize_system_prompt(project_name: &str) -> String {
    let name = project_name.trim();
    let name = if name.is_empty() { "unknown" } else { name };
    AGENTS_MD_OPTIMIZE_PROMPT
        .replace("{project_name}", name)
        .replace("{language}", crate::uilang::reply_language())
        .replace("{unknown}", crate::owner_text::unknown())
}

/// 答问卡里一轮问答（2026-09-25 答问优化流）。问题与选项由
/// `brief_questions` 起草，答案由负责人在界面上选定或手填。
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct BriefQA {
    pub question: String,
    pub answer: String,
}

/// 答问卡里的一道题：题干 + 可选答案（界面另有「其他」手填口）。
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct BriefQuestion {
    pub question: String,
    pub options: Vec<String>,
}

/// AI 起草的单角色种子：职责一段（能力域+显式边界+协作线索）。
/// 落进向导 `roleOverrides` 的 duty 字段，其余字段（model_slot/reviewer/
/// skills/globs）沿用模板生效值，不回写模板库。
/// `globs` 恒空（ADR 0075）：新项目还没有目录结构，起草产 globs 纯属
/// 虚构，填错比空更糟——会在之后的真实写入上反复误触发越权裁决。
/// 字段保留只为 DTO 兼容；解析器不再读它。
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct RoleSeedDraft {
    pub name: String,
    pub duty: String,
    pub globs: Vec<String>,
}

/// 一句话 → 项目说明草稿。调用刚配好的主对话模型（`default` 槽），
/// 不写磁盘：本函数不接收目录。落盘只走 `write_agents_md`，且只在负责人确认后。
///
/// 票 16 / ADR 0067。被否决的替代：模型一返回就写盘——取消或后退会留下半份说明。
/// 不给工具：空目录没有可核对的命令，给了工具模型会去猜技术栈。
/// 模型若仍编造，草稿停在文本框里，确认前可以改，不会进仓库
/// （false negative 花一次人工编辑）。程序再改模型正文会毁掉
/// 「用用户那句话的语言」（false positive），所以正文原样返回，约束放在提示词里。
/// `qa` 是答问流（`brief_questions` + 界面答问卡）收集的答案，拼进用户消息。
pub fn optimize_agents_md(
    project_name: &str,
    sentence: &str,
    qa: &[BriefQA],
    provider: &dyn ModelProvider,
) -> Result<String, SetupError> {
    let sentence = sentence.trim();
    if sentence.is_empty() {
        return Err(SetupError::EmptyBrief);
    }
    let mut user = sentence.to_string();
    if !qa.is_empty() {
        user.push_str("\n\nThe owner also answered these clarifying questions:");
        for a in qa {
            user.push_str(&format!("\n- Q: {}\n  A: {}", a.question, a.answer));
        }
    }
    let req = ChatRequest {
        // 主对话模型 = 票 13 放行的 default 槽，不是某个角色槽。
        model_slot: "default".into(),
        messages: vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: optimize_system_prompt(project_name),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: user }],
            },
        ],
        tools: vec![],
    };
    let resp = provider.complete(&req)?;
    let text: String = resp
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if text.trim().is_empty() {
        return Err(SetupError::EmptyDraft);
    }
    Ok(text)
}

/// 答问提示词（2026-09-25 答问优化流）：在起草 AGENTS.md 前先问能改写
/// 文件的问题——技术选型、服务对象、边界、硬约束。英文模板，题目用界面语言。
/// 占位 `{project_name}`、`{language}`。
/// 题数不设上限（owner 2026-09-25）：「最多 3 题」曾漏掉技术选型这种关键题，
/// 宁可多问让负责人滚动作答，也不能为凑短漏题。
pub const BRIEF_QUESTIONS_PROMPT: &str = "You interview the owner before AGENTS.md is drafted for their project \"{project_name}\". Their one-sentence idea is in the user message. Ask every question whose answer would meaningfully change that file — typically intended tech stack, who it is for, scope boundaries, hard constraints (platform, integrations, offline). Never drop a real question just to keep the list short, but skip anything the sentence already answers. Every question offers 2 to 4 short, concrete options a non-technical owner can pick between. Write questions and options in {language}. Output a JSON array only, like [{\"question\":\"...\",\"options\":[\"...\",\"...\"]}]. No explanation.";

fn brief_questions_prompt(project_name: &str) -> String {
    let name = project_name.trim();
    let name = if name.is_empty() { "unknown" } else { name };
    BRIEF_QUESTIONS_PROMPT
        .replace("{project_name}", name)
        .replace("{language}", crate::uilang::reply_language())
}

/// 一句话 → 答问题目草稿（题数不设上限，每题选项封顶 4）。走项目说明槽；
/// 界面把题渲染成答问卡（右侧滚动列表），答案随 `optimize_agents_md` 的 `qa` 回流。
/// 模型答不上/答非 JSON 报错——调用方回落到无答问的直出草稿。
pub fn brief_questions(
    project_name: &str,
    sentence: &str,
    provider: &dyn ModelProvider,
) -> Result<Vec<BriefQuestion>, SetupError> {
    let sentence = sentence.trim();
    if sentence.is_empty() {
        return Err(SetupError::EmptyBrief);
    }
    let req = ChatRequest {
        model_slot: crate::provider_config::BRIEF_SLOT.into(),
        messages: vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: brief_questions_prompt(project_name),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: sentence.to_string(),
                }],
            },
        ],
        tools: vec![],
    };
    let resp = provider.complete(&req)?;
    parse_brief_questions(&crate::intake::response_text(&resp))
}

/// 解析答问 JSON：剥 ``` 围栏、取首个 `[` 到末个 `]`，逐题打捞
/// （题干非空 + 至少一个非空选项才收）。题数不设上限，选项封顶 4 项。
/// 空数组合法 = 模型判断没有可问的，界面直出草稿。
fn parse_brief_questions(text: &str) -> Result<Vec<BriefQuestion>, SetupError> {
    let text = text.trim();
    let inner = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(|rest| rest.trim_end_matches('`').trim())
        .unwrap_or(text);
    let (Some(start), Some(end)) = (inner.find('['), inner.rfind(']')) else {
        return Err(SetupError::BadQuestions(inner.chars().take(80).collect()));
    };
    if end < start {
        return Err(SetupError::BadQuestions(inner.chars().take(80).collect()));
    }
    let items: Vec<serde_json::Value> = serde_json::from_str(&inner[start..=end])
        .map_err(|e| SetupError::BadQuestions(e.to_string()))?;
    let mut out: Vec<BriefQuestion> = Vec::new();
    for item in items {
        let question = item
            .get("question")
            .and_then(|q| q.as_str())
            .unwrap_or("")
            .trim();
        let options: Vec<String> = item
            .get("options")
            .and_then(|o| o.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|o| {
                        o.as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                    })
                    .take(4)
                    .collect()
            })
            .unwrap_or_default();
        if question.is_empty() || options.is_empty() {
            continue;
        }
        out.push(BriefQuestion {
            question: question.to_string(),
            options,
        });
    }
    if out.is_empty() && inner[start..=end].trim() != "[]" {
        return Err(SetupError::BadQuestions("no salvageable question".into()));
    }
    Ok(out)
}

/// 角色种子起草提示词：按项目说明把每个角色的职责改写成一段话
/// （能力域、显式边界、协作线索各一句上下；ADR 0075）。协作线索是
/// 软提示不是路由规则——网状调度仍在阶段名单/点名/失速机制层，
/// 不让职责文本固化交接顺序。不再产归属 globs：空目录上的
/// globs 纯属虚构。英文模板，职责用界面语言。占位 `{language}`。
pub const ROLE_SEEDS_PROMPT: &str = "You tailor a software team's role definitions to one project. The user message is JSON: {\"brief\": project description, \"roles\": [{\"name\", \"duty\"}]}. For each role, rewrite the duty into a short paragraph in {language} of two to four sentences: what the role owns in this project (capability scope), what it does not do (explicit boundary), and when it should escalate or pull in another kind of role (a collaboration cue — a hint, not a routing rule; never fix a handoff order). Every item must reach that depth — a duty compressed to a single clause is a defect, and later roles must not be shallower than earlier ones. Keep each input name exactly once. Output a JSON array only: [{\"name\":\"...\",\"duty\":\"...\"}]. No explanation.";

/// 按项目说明批量起草勾选角色的职责段落。走角色起草槽；
/// 只回草稿——界面对每角色落 `roleOverrides`（已定制的角色不入参，不覆盖）。
/// 输出按入参角色名过滤、先到先得去重；模型编造的角色名丢弃。
/// 模型若违提示仍产出 globs，解析时一律丢弃（ADR 0075）。
pub fn draft_role_defs(
    brief: &str,
    roles: &[RoleDef],
    provider: &dyn crate::provider::ModelProvider,
) -> Result<Vec<RoleSeedDraft>, SetupError> {
    if roles.is_empty() {
        return Ok(vec![]);
    }
    // 一次塞进十几个角色时，模型只改写第一项，后面几项抄旧稿或停在模板上
    // （owner 2026-09-25：魂斗罗只出现在产品策划）。四个人一组，组内再走薄稿补跑。
    const CHUNK: usize = 4;
    if roles.len() > CHUNK {
        let mut all = Vec::new();
        for chunk in roles.chunks(CHUNK) {
            all.extend(draft_role_defs(brief, chunk, provider)?);
        }
        return Ok(all);
    }
    let brief = brief.trim();
    if brief.is_empty() {
        return Err(SetupError::EmptyBrief);
    }
    let t0 = std::time::Instant::now();
    let mut seeds = draft_seeds_once(brief, roles, provider).inspect_err(|_| {
        crate::diag::note(
            crate::diag::CLASS_HOST,
            true,
            None,
            None,
            None,
            None,
            "role_draft",
            "batch_err",
            t0,
        );
    })?;
    // 薄稿修复（owner 实测 2026-09-25）：JSON 数组起草常见位置退化——
    // 首项写足段落、余项压回一行。薄稿与漏稿角色收成子集、同提示词
    // 补跑一轮（输入变短退化压力更小）。只修一轮不循环：仍薄落的也是
    // 可编辑草稿，界面还有「重新起草」可再试。
    let deficient: Vec<RoleDef> = roles
        .iter()
        .filter(|r| {
            seeds
                .iter()
                .find(|s| s.name == r.name)
                .map(|s| {
                    let src = roles
                        .iter()
                        .find(|r| r.name == s.name)
                        .map(|r| r.duty.as_str())
                        .unwrap_or("");
                    duty_is_thin(&s.duty, src)
                })
                .unwrap_or(true) // 漏稿
        })
        .cloned()
        .collect();
    if deficient.is_empty() {
        crate::diag::host("role_draft", "clean", t0);
        return Ok(seeds);
    }
    let n = deficient.len();
    let repaired = match draft_seeds_once(brief, &deficient, provider) {
        Ok(r) => r,
        Err(_) => {
            crate::diag::note(
                crate::diag::CLASS_HOST,
                true,
                None,
                None,
                None,
                None,
                "role_draft",
                "repair_err",
                t0,
            );
            vec![]
        }
    };
    for fix in repaired {
        match seeds.iter_mut().find(|s| s.name == fix.name) {
            // 薄稿换薄稿不换；漏稿补位则薄也收（有总比没有强）
            Some(_)
                if duty_is_thin(
                    &fix.duty,
                    roles
                        .iter()
                        .find(|r| r.name == fix.name)
                        .map(|r| r.duty.as_str())
                        .unwrap_or(""),
                ) => {}
            Some(slot) => *slot = fix,
            None => seeds.push(fix),
        }
    }
    crate::diag::host("role_draft", &format!("repair:{n}"), t0);
    Ok(seeds)
}

fn draft_seeds_once(
    brief: &str,
    roles: &[RoleDef],
    provider: &dyn crate::provider::ModelProvider,
) -> Result<Vec<RoleSeedDraft>, SetupError> {
    let input = serde_json::json!({
        "brief": brief,
        "roles": roles.iter().map(|r| serde_json::json!({ "name": r.name, "duty": r.duty })).collect::<Vec<_>>(),
    });
    let req = crate::provider::ChatRequest {
        model_slot: crate::provider_config::ROLE_DRAFT_SLOT.into(),
        messages: vec![
            crate::provider::Message {
                role: crate::provider::Role::System,
                content: vec![crate::provider::ContentBlock::Text {
                    text: ROLE_SEEDS_PROMPT.replace("{language}", crate::uilang::reply_language()),
                }],
            },
            crate::provider::Message {
                role: crate::provider::Role::User,
                content: vec![crate::provider::ContentBlock::Text {
                    text: input.to_string(),
                }],
            },
        ],
        tools: vec![],
    };
    let resp = provider.complete(&req)?;
    parse_role_seeds(&crate::intake::response_text(&resp), roles)
}

/// 薄稿判定：少于两个句读收尾符即视为被压缩的一行稿。段末句号常省，
/// 两句稿可能只数到一——宁多修一轮（有界：一轮），不放薄稿过关。
fn duty_is_thin(duty: &str, source: &str) -> bool {
    // 句读不够 = 被压成一行。与模板职责相同（或只是模板原文再套一句）
    // 也算薄：首项写足、余项照抄模板时，卡面上看起来「只有产品策划变了」。
    if duty.matches(['。', '！', '？', '!', '?', '.']).count() < 2 {
        return true;
    }
    let src = source.trim();
    if src.is_empty() {
        return false;
    }
    let body = duty.trim();
    body == src || (body.contains(src) && body.chars().count() < src.chars().count() + 12)
}

fn parse_role_seeds(text: &str, roles: &[RoleDef]) -> Result<Vec<RoleSeedDraft>, SetupError> {
    let text = text.trim();
    let inner = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(|rest| rest.trim_end_matches('`').trim())
        .unwrap_or(text);
    let (Some(start), Some(end)) = (inner.find('['), inner.rfind(']')) else {
        return Err(SetupError::BadRoleDefs(inner.chars().take(80).collect()));
    };
    if end < start {
        return Err(SetupError::BadRoleDefs(inner.chars().take(80).collect()));
    }
    let items: Vec<serde_json::Value> = serde_json::from_str(&inner[start..=end])
        .map_err(|e| SetupError::BadRoleDefs(e.to_string()))?;
    let mut out: Vec<RoleSeedDraft> = Vec::new();
    for item in items {
        let name = item
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .trim();
        // 只收入参名（模型编造的角色名丢弃）；先到先得去重。
        if name.is_empty()
            || !roles.iter().any(|r| r.name == name)
            || out.iter().any(|o| o.name == name)
        {
            continue;
        }
        let duty = item
            .get("duty")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .trim();
        // ADR 0075：起草不产 globs（新项目无目录结构，产出纯属虚构，
        // 填错会在真实写入上误触发越权裁决）。模型若仍带 globs 字段
        // 一律丢弃——空 = 不做归属检查，由运行期提案/手动收窄。
        out.push(RoleSeedDraft {
            name: name.to_string(),
            duty: duty.to_string(),
            globs: vec![],
        });
    }
    Ok(out)
}

/// 流程起草提示词（ADR 0071：英文，阶段名用界面语言）。
pub(crate) fn flow_prompt(sentence: &str) -> String {
    let roles = preset_roles()
        .map(|rs| {
            rs.into_iter()
                .filter(|r| r.name != crate::pm_route::PM_ROLE)
                .map(|r| r.name)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    format!(
        "From the project description below, output one process pack as JSON and nothing else. \
         Fields: name, version, stages. version is an integer (1), not a dotted string. \
         Each stage has name, roles, due, stamp_point. \
         roles is an array of role names chosen only from: {roles}. \
         Do not put 项目经理 in any stage roles. He dispatches; he is not activated to produce artifacts. \
         due is an array of artifact kind names, not a date or a duration. \
         stamp_point is a boolean, true or false. \
         Write stage names in {}. No explanation.\n\n{sentence}",
        crate::uilang::reply_language()
    )
}

/// 职责起草提示词。旧版写死「起草一段中文职责」；ADR 0071 起跟随界面语言。
/// ADR 0075：从一行升级为 2–4 句段落——能力域、显式边界、协作线索。
pub(crate) fn duty_prompt(name: &str, hint: &str) -> String {
    format!(
        "Draft the duty statement for the role \"{name}\" in {}: a short paragraph of two to four sentences — what the role owns, what it does not do, and when it should escalate or pull in other roles. Output only the statement. Extra guidance: {hint}",
        crate::uilang::reply_language()
    )
}

/// 按项目说明起草流程包。调用方传入流程起草槽（未绑则已落到默认槽）的供应商。
/// 只返回草稿，不写盘。检验命令、产物清单和回填边不在这张草稿的编辑面上。
pub fn draft_flow(
    sentence: &str,
    provider: &dyn crate::provider::ModelProvider,
) -> Result<crate::orchestra::PackDef, SetupError> {
    let sentence = sentence.trim();
    if sentence.is_empty() {
        return Err(SetupError::EmptyBrief);
    }
    let req = crate::provider::ChatRequest {
        model_slot: crate::provider_config::FLOW_DRAFT_SLOT.into(),
        messages: vec![crate::provider::Message {
            role: crate::provider::Role::User,
            content: vec![crate::provider::ContentBlock::Text {
                text: flow_prompt(sentence),
            }],
        }],
        tools: vec![],
    };
    let resp = provider.complete(&req)?;
    parse_flow_draft(&crate::intake::response_text(&resp))
}

/// 2026-09-24 活测：`flow_draft`（qwen3.7-plus）把 version 写成 `"1.0"` /
/// `"1.0.0"`，又把 due 写成 `"2天"` 或 `"2024-06-01"`。提示词现在写明类型，
/// 这里仍把主版本号收成整数，把本该是字符串数组的单值收成一项。
/// 钉住的流程包副本不走这条，继续严格反序列化。
fn parse_flow_draft(text: &str) -> Result<crate::orchestra::PackDef, SetupError> {
    let text = text.trim();
    let json_text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(|rest| rest.trim_end_matches('`').trim())
        .unwrap_or(text);
    let mut v: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| SetupError::BadFlow(e.to_string()))?;
    if let Some(ver) = v.get_mut("version") {
        if let Some(s) = ver.as_str() {
            let major = s.split(['.', '-']).next().unwrap_or(s);
            let n: u32 = major
                .parse()
                .map_err(|_| SetupError::BadFlow(format!("version {s:?} is not an integer")))?;
            *ver = serde_json::json!(n);
        }
    }
    if let Some(stages) = v.get_mut("stages").and_then(|s| s.as_array_mut()) {
        for stage in stages {
            for key in ["roles", "due", "checks", "consult_wake"] {
                let Some(field) = stage.get_mut(key) else {
                    continue;
                };
                if let Some(s) = field.as_str() {
                    *field = serde_json::json!([s]);
                }
            }
            if let Some(roles) = stage.get_mut("roles").and_then(|r| r.as_array_mut()) {
                roles.retain(|r| r.as_str() != Some(crate::pm_route::PM_ROLE));
            }
        }
    }
    serde_json::from_value(v).map_err(|e| SetupError::BadFlow(e.to_string()))
}

/// 向导在项目还没建时读已有说明。只接受这两个文件名，不扫目录。
pub fn read_instruction_file(dir: &Path) -> Result<String, SetupError> {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(std::fs::read_to_string(path)?);
        }
    }
    Ok(String::new())
}

/// 角色起草槽优化职责。人确认才写入模板。
pub fn draft_role_duty(
    name: &str,
    hint: &str,
    provider: &dyn crate::provider::ModelProvider,
) -> Result<String, SetupError> {
    let req = crate::provider::ChatRequest {
        model_slot: crate::provider_config::ROLE_DRAFT_SLOT.into(),
        messages: vec![crate::provider::Message {
            role: crate::provider::Role::User,
            content: vec![crate::provider::ContentBlock::Text {
                text: duty_prompt(name, hint),
            }],
        }],
        tools: vec![],
    };
    let resp = provider.complete(&req)?;
    let text = crate::intake::response_text(&resp);
    if text.trim().is_empty() {
        return Err(SetupError::EmptyDraft);
    }
    Ok(text)
}

/// 写 `AGENTS.md`。已有项目说明（`AGENTS.md` 或 `CLAUDE.md`）即拒绝，两份都不改。
///
/// 票 16 / ADR 0067：没有 AGENTS.md 时 CLAUDE.md 就是项目说明。旁边再写一份
/// AGENTS.md 会被 `inspect_dir` 当成主文件，等于换掉现成约束。
/// false negative（漏拒）花一次未审覆盖；false positive（CLAUDE.md 在时
/// 拒绝另写 AGENTS.md）花一次人工。偏向拒绝。内容相同也报——不静默跳过。
pub fn write_agents_md(dir: impl AsRef<Path>, content: &str) -> Result<(), SetupError> {
    let dir = dir.as_ref();
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let existing = dir.join(name);
        if existing.exists() {
            return Err(SetupError::AgentsMdExists(existing));
        }
    }
    std::fs::write(dir.join("AGENTS.md"), content)?;
    Ok(())
}

/// 所选角色的模型槽里哪些不就绪（返回槽位名，去重排序）。
/// 就绪 = 槽位有绑定 + 供应商存在且启用 + key 已存（default 槽兜底）——
/// 语义已从「缺 key」升级为「缺可用供应商」，见 provider_config.rs。
pub fn missing_model_keys(
    store: &dyn CredentialStore,
    roles: &[RoleDef],
    doc: &crate::provider_config::ProviderDoc,
) -> Result<Vec<String>, SetupError> {
    let mut missing = Vec::new();
    for slot in {
        let mut s: Vec<&str> = roles.iter().map(|r| r.model_slot.as_str()).collect();
        s.sort();
        s.dedup();
        s
    } {
        if !crate::provider_config::slot_ready(doc, store, slot) {
            missing.push(slot.to_string());
        }
    }
    Ok(missing)
}

/// 向导最后一步的可见进度（票 14）。顺序即 `CREATE_STEP_ORDER`：
/// 检查目录 → git → 角色落库 → 密钥复查 → 打开项目。
/// 只在该步成功结束后报告；失败的那一步以及后面的步都不报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum CreateStep {
    /// 目录已就绪（建得出来、是目录）。
    CheckDir,
    /// git 闸已过：已是干净仓库，或负责人确认后完成初始化。
    Git,
    /// 角色定义已写入项目库（model_slot / globs / grants）。
    PersistRoles,
    /// 所选角色的模型槽复查通过。
    RecheckKeys,
    /// 工作台实例已建好，可以交给壳层打开。
    OpenProject,
}

/// 创建进度的固定顺序。界面清单与测试都钉这一份，不另排。
pub const CREATE_STEP_ORDER: [CreateStep; 5] = [
    CreateStep::CheckDir,
    CreateStep::Git,
    CreateStep::PersistRoles,
    CreateStep::RecheckKeys,
    CreateStep::OpenProject,
];

/// `create_project` 命令的入参 DTO。原本在 src-tauri 定义，但 ts-rs 跨 crate
/// 引用时把依赖类型的 `export_to` 原样写进 import 路径（src-tauri 的
/// `../../ui/src/gen/` × 依赖的 `../../../ui/src/gen/` 拼出仓外路径，
/// 类型检查只靠邻目录的巧合文件通过）。定义挪到与 RoleDef/PackDef 同 crate，
/// import 永远生成 `./`。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct CreateProjectOpts {
    pub dir: String,
    pub name: String,
    pub roles: Vec<String>,
    /// 角色定制覆盖（ADR 0057）：自定义模板与向导改过的角色传完整定义；
    /// 未列名字走内置目录。壳层不读全局模板文件，保持 core 纯函数。
    #[serde(default)]
    #[ts(optional)]
    pub role_overrides: Option<Vec<RoleDef>>,
    #[ts(optional)]
    pub pack_name: Option<String>,
    #[ts(optional)]
    pub fastpath_role: Option<String>,
    pub init_git: bool,
    #[ts(optional)]
    pub agents_md: Option<String>,
    /// 向导生成的流程草稿。有它就不用预置包名字。
    #[serde(default)]
    #[ts(optional)]
    pub pack: Option<PackDef>,
    /// 已废弃。传了任何档，核都拒绝且不建项目（ADR 0069）。
    #[serde(default)]
    #[ts(optional)]
    pub autonomy: Option<String>,
}

/// 建项目（向导最后一步）。`roles` 是模板目录（内置∪自定义）角色名子集；
/// `role_overrides` 携带**完整定义**——自定义模板与向导改过的角色都经它传入，
/// 本函数不读全局模板文件（保持纯函数，测试不被 HOME 污染）。`pack` 为 None 时
/// `fastpath_role` 必须给（快速通道）；`init_git` = 负责人确认了「无 git 则初始化」。
/// `autonomy`：必须是 `None`。传了任何档（包括原先合法的 L0–L4）都在写盘前拒绝。
/// 全程 fail-closed：已有工作台停（再建会改写花名册）、缺密钥停、未知角色停、野 override 停。
/// 脏树不停——ADR 0060：未提交改动留给负责人，开项目不提交、不清理。
/// 进度走 `create_project_reporting`；本函数不报告（脚本/测试旧入口）。
#[allow(clippy::too_many_arguments)]
pub fn create_project(
    dir: impl AsRef<Path>,
    name: &str,
    roles: &[String],
    role_overrides: &[RoleDef],
    pack: Option<&PackDef>,
    fastpath_role: Option<&str>,
    init_git: bool,
    store: &dyn CredentialStore,
    doc: &crate::provider_config::ProviderDoc,
    autonomy: Option<&str>,
) -> Result<Workbench, SetupError> {
    create_project_reporting(
        dir,
        name,
        roles,
        role_overrides,
        pack,
        fastpath_role,
        init_git,
        store,
        doc,
        autonomy,
        None,
        |_| {},
    )
}

/// 同 `create_project`，但每一步**做完**就调用 `on_step`。
/// 壳层把回调推进 IPC channel，界面才不会等到整段结束才有反应（票 14）。
/// 被否决的替代：界面用定时器按清单打勾——那和真实步骤脱节，失败也会假装往后走。
#[allow(clippy::too_many_arguments)]
pub fn create_project_reporting<F>(
    dir: impl AsRef<Path>,
    name: &str,
    roles: &[String],
    role_overrides: &[RoleDef],
    pack: Option<&PackDef>,
    fastpath_role: Option<&str>,
    init_git: bool,
    store: &dyn CredentialStore,
    doc: &crate::provider_config::ProviderDoc,
    autonomy: Option<&str>,
    agents_md: Option<&str>,
    mut on_step: F,
) -> Result<Workbench, SetupError>
where
    F: FnMut(CreateStep),
{
    if autonomy.is_some() {
        // ADR 0069：向导不再接收档位。传了就整单拒绝，目录还没建。
        return Err(SetupError::Autonomy(
            crate::autonomy::AutonomyError::GearsRemoved,
        ));
    }
    let dir = dir.as_ref();
    // 票 17：空不空看写说明文件之前。壳层确认过的一句话会在下面才落盘，
    // 若先写再看，空目录会变成「已有 AGENTS.md」从而误走开场分析。
    // 被否决：用「只有说明文件」反推空目录——只有一份现成 AGENTS.md 的仓库
    // 和刚写进空目录的简报分不清。
    let needs_intake = opening_intake_needed(dir);
    // ADR 0060：已有工作台状态是打开，不是再建。先于 mkdir / git init，
    // 避免二次创建改 mode、槽位、授权。被否决的替代：静默当成 open 并套用
    // 本次向导选项（会改写已有项目）。false negative（漏判已有库）会重跑落库；
    // 本闸偏向拒绝再建。
    if dir.join(".hexagon/state.db").is_file() {
        return Err(SetupError::AlreadyProject(dir.to_path_buf()));
    }
    std::fs::create_dir_all(dir)?;
    // 负责人已确认的说明。已有 AGENTS.md / CLAUDE.md 时 write 拒绝，不覆盖。
    // 放在进度回调之前：写失败等于这一步还没完成，界面不该亮「检查目录」。
    if let Some(md) = agents_md {
        write_agents_md(dir, md)?;
    }
    on_step(CreateStep::CheckDir);

    // git 闸：非仓库须确认后才 init。已是仓库则无论干净或有未提交改动都直接用：
    // 不 init、不 commit、不 clean（ADR 0060 弃「脏树先停」；自动提交/stash 会动工作区）。
    // 失败不报告 Git，调用方停在这一步。
    if !git::is_repo(dir) {
        if init_git {
            git::init(dir, "main")?;
        } else {
            return Err(SetupError::NoGit(dir.to_path_buf()));
        }
    }
    on_step(CreateStep::Git);

    // 解析角色：override 优先（自定义模板/向导定制），回落内置（未知角色拒绝）。
    // override 校验只做定义级（名非空/不自审/上级在目录内）——技能名不验，
    // 理由同 templates.rs 头注；上级目录级校验同内置预置，不要求已勾选。
    let presets = preset_roles()?;
    let known: std::collections::HashSet<&str> = presets
        .iter()
        .chain(role_overrides.iter())
        .map(|d| d.name.as_str())
        .collect();
    for o in role_overrides {
        if !roles.contains(&o.name) {
            return Err(SetupError::StrayOverride(o.name.clone()));
        }
        if o.name.trim().is_empty() {
            return Err(SetupError::UnknownRole(o.name.clone()));
        }
        if o.reviewer.as_deref() == Some(o.name.as_str()) {
            return Err(SetupError::SelfReviewer(o.name.clone()));
        }
        if let Some(rev) = &o.reviewer {
            if !known.contains(rev.as_str()) {
                return Err(SetupError::UnknownOverrideReviewer {
                    role: o.name.clone(),
                    reviewer: rev.clone(),
                });
            }
        }
    }
    let mut picked: Vec<RoleDef> = Vec::new();
    for r in roles {
        if let Some(o) = role_overrides.iter().find(|o| &o.name == r) {
            picked.push(o.clone());
        } else {
            picked.push(
                presets
                    .iter()
                    .find(|p| &p.name == r)
                    .ok_or_else(|| SetupError::UnknownRole(r.clone()))?
                    .clone(),
            );
        }
    }
    if pack.is_none() {
        match fastpath_role {
            Some(fr) if picked.iter().any(|p| p.name == fr) => {}
            _ => return Err(SetupError::NoFastRole),
        }
    }

    // 可见顺序是「角色落库 → 密钥复查」（票 14）。缺 key 仍不能留下项目：
    // 新建目录落库后再复查，失败则删掉本次的 .hexagon（返回时没有 state.db）。
    // 目录里本来就有 state.db 时不能这么删——那会误伤已有工作台，所以复查
    // 提前到落库之前，缺 key 直接停、不改库。被否决的替代：缺 key 也留着半成品库。
    let db_path = dir.join(".hexagon/state.db");
    let db_existed = db_path.exists();
    if db_existed {
        let missing = missing_model_keys(store, &picked, doc)?;
        if !missing.is_empty() {
            return Err(SetupError::MissingKeys(missing));
        }
    }
    let hex_existed = dir.join(".hexagon").exists();

    // 开项目 + 实例化角色（model_slot / globs / 技能授权）
    let pairs: Vec<(String, String)> = picked
        .iter()
        .enumerate()
        .map(|(i, r)| (format!("a{i}"), r.name.clone()))
        .collect();
    let persisted = (|| -> Result<Workbench, SetupError> {
        let wb = Workbench::open(dir, name, &pairs, pack.cloned())?;
        let _ = autonomy;
        let db = Db::open(&db_path)?;
        let mode = if pack.is_some() { "pack" } else { "fastpath" };
        let fast_agent = fastpath_role.map(|fr| {
            pairs
                .iter()
                .find(|(_, r)| r == fr)
                .map(|(a, _)| a.clone())
                .unwrap_or_default()
        });
        db.conn().execute(
            "UPDATE projects SET mode=?1, pack_name=?2, pack_copy_version=?3, fastpath_agent_id=?4 WHERE id='p1'",
            rusqlite::params![
                mode,
                pack.map(|p| p.name.as_str()),
                pack.map(|p| p.version as i64),
                fast_agent,
            ],
        )?;
        for ((aid, _), def) in pairs.iter().zip(picked.iter()) {
            db.conn().execute(
                "UPDATE agents SET model_slot=?1 WHERE id=?2",
                rusqlite::params![def.model_slot, aid],
            )?;
            for g in &def.globs {
                db.conn().execute(
                    "INSERT OR IGNORE INTO agent_globs (agent_id, glob) VALUES (?1, ?2)",
                    rusqlite::params![aid, g],
                )?;
            }
            for s in &def.skills {
                db.conn().execute(
                    "INSERT OR IGNORE INTO grants (id, agent_id, kind, name)
                     VALUES (?1, ?2, 'skill', ?3)",
                    rusqlite::params![format!("g-{aid}-{s}"), aid, s],
                )?;
            }
        }
        Ok(wb)
    })();
    let wb = match persisted {
        Ok(wb) => wb,
        Err(e) => {
            rollback_fresh_hexagon(dir, hex_existed);
            return Err(e);
        }
    };
    if needs_intake {
        if let Err(e) = wb.db.conn().execute(
            "UPDATE projects SET opening_intake=?1 WHERE id='p1'",
            [crate::intake::STATUS_PENDING],
        ) {
            drop(wb);
            rollback_fresh_hexagon(dir, hex_existed);
            return Err(e.into());
        }
    }
    on_step(CreateStep::PersistRoles);

    let missing = missing_model_keys(store, &picked, doc)?;
    if !missing.is_empty() {
        drop(wb);
        rollback_fresh_hexagon(dir, hex_existed);
        return Err(SetupError::MissingKeys(missing));
    }
    on_step(CreateStep::RecheckKeys);
    // 工作台对象已经在手里。壳层随后才把它装进窗口；这一步失败则命令报错，
    // 界面不得因为本回调而进入工作台。
    on_step(CreateStep::OpenProject);
    Ok(wb)
}

/// 打开已有项目（ADR 0060）。只接受已有 `.hexagon/state.db` 的目录。
/// 不初始化 git、不提交、不清理工作区，不重跑角色落库（空角色表 + INSERT OR IGNORE）。
/// 被否决的替代：没有状态也 `Workbench::open`（那会把空目录建成新库）。
pub fn open_existing(dir: impl AsRef<Path>) -> Result<Workbench, SetupError> {
    let dir = dir.as_ref();
    if !dir.join(".hexagon/state.db").is_file() {
        return Err(SetupError::NotAProject(dir.to_path_buf()));
    }
    let pack = PackDef::pinned(dir).ok();
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    Ok(Workbench::open(dir, name, &[], pack)?)
}

/// 目录在成为项目之前就已经有条目，且还不是工作台。空目录、还不存在的
/// 目录、已经是项目的目录，都不走开场分析。
fn opening_intake_needed(dir: &Path) -> bool {
    let report = inspect_dir(dir);
    report.exists && !report.empty && !report.has_workbench
}

/// 落库后失败：本次新建的 `.hexagon` 整目录删掉，避免缺密钥却留下项目库。
/// 目录事先就有 `.hexagon` 时不动（里面可能是已有工作台）。
fn rollback_fresh_hexagon(dir: &Path, hex_existed: bool) {
    if hex_existed {
        return;
    }
    let _ = std::fs::remove_dir_all(dir.join(".hexagon"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;
    use serde_json::json;

    fn store_with_key() -> MemoryStore {
        let s = MemoryStore::default();
        s.set("provider/test-prov", "sk-test").unwrap();
        s
    }

    /// 测试用供应商档：test-prov + chat 槽绑定（新就绪语义=绑定+启用+key）。
    fn doc_with_provider() -> crate::provider_config::ProviderDoc {
        let mut doc = crate::provider_config::ProviderDoc::default();
        doc.providers.push(crate::provider_config::ProviderDef {
            id: "test-prov".into(),
            name: "Test".into(),
            kind: crate::provider::ProviderKind::OpenAi,
            base_url: "http://localhost".into(),
            models: vec![],
            enabled: true,
        });
        doc.slots.insert(
            "chat".into(),
            crate::provider_config::SlotBinding {
                provider_id: "test-prov".into(),
                model: "m".into(),
            },
        );
        doc
    }

    fn pack() -> PackDef {
        serde_json::from_value(json!({
            "name":"规格驱动","version":1,
            "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"],"stamp_point":true}]
        }))
        .unwrap()
    }

    #[test]
    fn flow_draft_accepts_dotted_version_string() {
        for raw in [r#""1.0""#, r#""1.0.0""#, "1"] {
            let text = format!(
                r#"{{"name":"论坛","version":{raw},"stages":[{{"name":"需求","roles":["产品策划"],"due":["规格"],"stamp_point":true}}]}}"#
            );
            let pack = parse_flow_draft(&text).unwrap();
            assert_eq!(pack.version, 1, "{raw}");
        }
        let err = parse_flow_draft(r#"{"name":"论坛","version":"beta","stages":[]}"#).unwrap_err();
        assert!(matches!(err, SetupError::BadFlow(_)), "{err}");
        let dated = parse_flow_draft(
            r#"{"name":"论坛","version":"1.0","stages":[{"name":"需求","roles":"产品策划","due":"2024-06-01","stamp_point":true}]}"#,
        )
        .unwrap();
        assert_eq!(dated.stages[0].due, vec!["2024-06-01".to_string()]);
        assert_eq!(dated.stages[0].roles, vec!["产品策划".to_string()]);
        let prompt = flow_prompt("论坛");
        assert!(prompt.contains("产品策划"), "{prompt}");
        assert!(prompt.contains("stamp_point is a boolean"), "{prompt}");
        assert!(prompt.contains("Do not put 项目经理"), "{prompt}");
        let stripped = parse_flow_draft(
            r#"{"name":"论坛","version":1,"stages":[{"name":"需求","roles":["产品策划","项目经理"],"due":["规格"],"stamp_point":true}]}"#,
        )
        .unwrap();
        assert_eq!(stripped.stages[0].roles, vec!["产品策划".to_string()]);
    }

    #[test]
    fn inspect_empty_dir() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("new-proj");
        let r = inspect_dir(&sub);
        assert!(!r.exists);
        std::fs::create_dir(&sub).unwrap();
        let r = inspect_dir(&sub);
        assert!(r.exists && r.empty && !r.is_git && !r.has_workbench && r.instructions.is_none());
    }

    #[test]
    fn create_empty_dir_with_git_init() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        let wb = create_project(
            &sub,
            "测试项目",
            &["产品策划".into(), "后端".into()],
            &[],
            Some(&pack()),
            None,
            true, // 确认初始化
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        assert!(git::is_repo(&sub));
        assert!(sub.join(".hexagon/pack.active.json").exists());
        let team = crate::orchestra::team(&wb.db, &wb.project_id, &wb.repo_root).unwrap();
        assert_eq!(team.len(), 2);
        // model_slot 和 globs 落库
        let db = Db::open(sub.join(".hexagon/state.db")).unwrap();
        let slot: String = db
            .conn()
            .query_row("SELECT model_slot FROM agents WHERE id='a0'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(slot, "chat");
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM agent_globs WHERE agent_id='a0'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n > 0);
        let skills: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM grants WHERE agent_id='a0' AND kind='skill'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(skills > 0);
    }

    /// ADR 0060：干净的非空 git 仓库仍可开成项目。
    #[test]
    fn clean_nonempty_repo_becomes_project() {
        let d = tempfile::tempdir().unwrap();
        git::init(d.path(), "main").unwrap();
        std::fs::write(d.path().join("README.md"), "kept").unwrap();
        git::commit_all(d.path(), "init").unwrap();
        let head = git::head(d.path()).unwrap();
        assert!(!git::is_dirty(d.path()));
        assert!(!inspect_dir(d.path()).empty);
        create_project(
            d.path(),
            "仓",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            false,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        assert!(d.path().join(".hexagon/state.db").is_file());
        assert_eq!(
            std::fs::read_to_string(d.path().join("README.md")).unwrap(),
            "kept"
        );
        // 开项目本身也不提交（.hexagon 留在工作区，不进这次 HEAD）
        assert_eq!(git::head(d.path()).unwrap(), head);
    }

    /// ADR 0060 弃「脏树先停」：未提交改动在创建后仍在，且没有被提交、没有被清掉。
    /// 旧断言是 DirtyTree——那条闸把已有代码的仓库挡在外面。
    #[test]
    fn dirty_repo_becomes_project_without_commit_or_clean() {
        let d = tempfile::tempdir().unwrap();
        git::init(d.path(), "main").unwrap();
        std::fs::write(d.path().join("README.md"), "base").unwrap();
        git::commit_all(d.path(), "init").unwrap();
        let head = git::head(d.path()).unwrap();
        std::fs::write(d.path().join("README.md"), "dirty-work").unwrap();
        std::fs::write(d.path().join("notes.txt"), "scratch").unwrap();
        create_project(
            d.path(),
            "仓",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            false,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(d.path().join("README.md")).unwrap(),
            "dirty-work"
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join("notes.txt")).unwrap(),
            "scratch"
        );
        assert_eq!(git::head(d.path()).unwrap(), head);
        let status = git::run(d.path(), &["status", "--porcelain"]).unwrap();
        assert!(
            status.lines().any(|l| l.contains("README.md")),
            "modified file must stay uncommitted, status={status}"
        );
        assert!(
            status.lines().any(|l| l.contains("notes.txt")),
            "untracked file must stay, status={status}"
        );
    }

    #[test]
    fn no_git_requires_confirm() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("x.txt"), "keep").unwrap(); // 非空非仓库
        assert!(matches!(
            create_project(
                d.path(),
                "p",
                &["产品策划".into()],
                &[],
                Some(&pack()),
                None,
                false,
                &store_with_key(),
                &doc_with_provider(),
                None,
            ),
            Err(SetupError::NoGit(_))
        ));
        assert!(!git::is_repo(d.path()));
        assert!(!d.path().join(".hexagon/state.db").exists());
        assert_eq!(
            std::fs::read_to_string(d.path().join("x.txt")).unwrap(),
            "keep"
        );
    }

    /// 确认后才 `git init`。初始化不把文件夹里已有文件提交进去。
    #[test]
    fn non_git_folder_inits_only_after_confirm() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("plain");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("x.txt"), "keep").unwrap();
        create_project(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        assert!(git::is_repo(&sub));
        assert_eq!(std::fs::read_to_string(sub.join("x.txt")).unwrap(), "keep");
        assert!(
            !git::run(&sub, &["ls-files"]).unwrap().contains("x.txt"),
            "git init must not commit the folder's existing files"
        );
    }

    /// 空目录同样未确认不能 init；确认后才成为仓库。
    #[test]
    fn empty_dir_requires_confirm_before_git_init() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("empty");
        std::fs::create_dir(&sub).unwrap();
        assert!(inspect_dir(&sub).empty);
        assert!(matches!(
            create_project(
                &sub,
                "p",
                &["产品策划".into()],
                &[],
                Some(&pack()),
                None,
                false,
                &store_with_key(),
                &doc_with_provider(),
                None,
            ),
            Err(SetupError::NoGit(_))
        ));
        assert!(!git::is_repo(&sub));
        create_project(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        assert!(git::is_repo(&sub));
        assert!(sub.join(".hexagon/state.db").is_file());
    }

    /// 已有工作台状态不能再建；打开不改花名册，也不提交/清理工作区。
    #[test]
    fn existing_workbench_refuses_recreate_and_opens() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("proj");
        let wb = create_project(
            &sub,
            "仓",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        drop(wb);
        std::fs::write(sub.join("README.md"), "still-dirty").unwrap();
        let head = git::head(&sub).unwrap();
        assert!(inspect_dir(&sub).has_workbench);
        assert!(matches!(
            create_project(
                &sub,
                "另一个",
                &["后端".into()],
                &[],
                Some(&pack()),
                None,
                true,
                &store_with_key(),
                &doc_with_provider(),
                None,
            ),
            Err(SetupError::AlreadyProject(_))
        ));
        assert_eq!(git::head(&sub).unwrap(), head);
        assert_eq!(
            std::fs::read_to_string(sub.join("README.md")).unwrap(),
            "still-dirty"
        );
        let opened = open_existing(&sub).unwrap();
        let n: i64 = opened
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let role: String = opened
            .db
            .conn()
            .query_row("SELECT role FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(role, "产品策划");
        let name: String = opened
            .db
            .conn()
            .query_row("SELECT name FROM projects WHERE id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "仓");
        assert!(opened.pack.is_some());
        assert_eq!(git::head(&sub).unwrap(), head);
        assert!(matches!(
            open_existing(d.path().join("missing")),
            Err(SetupError::NotAProject(_))
        ));
    }

    #[test]
    fn missing_key_blocks_start() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        let err = match create_project(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &MemoryStore::default(), // 没有任何 key
            &crate::provider_config::ProviderDoc::default(),
            None,
        ) {
            Ok(_) => panic!("missing key should block"),
            Err(e) => e,
        };
        match err {
            SetupError::MissingKeys(keys) => assert_eq!(keys, vec!["chat"]),
            e => panic!("expected MissingKeys, got {e}"),
        }
        // 项目没建起来
        assert!(!sub.join(".hexagon/state.db").exists());
    }

    #[test]
    fn fastpath_requires_checked_role() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        assert!(matches!(
            create_project(
                &sub,
                "p",
                &["产品策划".into()],
                &[],
                None,
                Some("后端"), // 未勾选
                true,
                &store_with_key(),
                &doc_with_provider(),
                None,
            ),
            Err(SetupError::NoFastRole)
        ));
        // 勾选后可走快速通道
        create_project(
            d.path().join("q"),
            "p",
            &["后端".into()],
            &[],
            None,
            Some("后端"),
            true,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        let db = Db::open(d.path().join("q/.hexagon/state.db")).unwrap();
        let mode: String = db
            .conn()
            .query_row("SELECT mode FROM projects WHERE id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "fastpath");
    }

    /// override：自定义模板名（不在内置目录）能选中并物化；定制字段落库；
    /// 野 override（未勾选角色）与自审/幽灵上级拒绝。
    #[test]
    fn role_overrides_materialize_and_validate() {
        let d = tempfile::tempdir().unwrap();
        let custom = RoleDef {
            name: "自建角色".into(),
            duty: "定制职责".into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec!["src/x/**".into()],
            skills: vec![],
        };
        // 自定义名不在内置目录 → 无 override 时 UnknownRole；有 override 时建得起来
        let sub = d.path().join("p");
        assert!(matches!(
            create_project(
                &sub,
                "p",
                &["自建角色".into()],
                &[],
                Some(&pack()),
                None,
                true,
                &store_with_key(),
                &doc_with_provider(),
                None,
            ),
            Err(SetupError::UnknownRole(_))
        ));
        let wb = create_project(
            &sub,
            "p",
            &["自建角色".into()],
            std::slice::from_ref(&custom),
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        let db = Db::open(sub.join(".hexagon/state.db")).unwrap();
        let role: String = db
            .conn()
            .query_row("SELECT role FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(role, "自建角色");
        let glob: String = db
            .conn()
            .query_row(
                "SELECT glob FROM agent_globs WHERE agent_id='a0'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(glob, "src/x/**");
        drop(wb);
        // 野 override / 自审 / 幽灵上级
        for (overrides, want) in [
            (vec![custom.clone()], "StrayOverride"),
            (
                vec![RoleDef {
                    reviewer: Some("自建角色".into()),
                    ..custom.clone()
                }],
                "SelfReviewer",
            ),
            (
                vec![RoleDef {
                    reviewer: Some("幽灵".into()),
                    ..custom.clone()
                }],
                "UnknownOverrideReviewer",
            ),
        ] {
            let sub = d.path().join(format!("{:?}", want));
            let roles = if want == "StrayOverride" {
                vec!["产品策划".into()]
            } else {
                vec!["自建角色".into()]
            };
            let e = match create_project(
                &sub,
                "p",
                &roles,
                &overrides,
                Some(&pack()),
                None,
                true,
                &store_with_key(),
                &doc_with_provider(),
                None,
            ) {
                Ok(_) => panic!("{want} should fail"),
                Err(e) => e,
            };
            assert_eq!(
                crate::errcode::variant_code(&e),
                match want {
                    "StrayOverride" => "stray_override",
                    "SelfReviewer" => "self_reviewer",
                    _ => "unknown_override_reviewer",
                }
            );
        }
    }

    /// 票 14：五步按做完的顺序报告，成功才包含「打开项目」。
    #[test]
    fn create_reports_each_step_when_it_finishes() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        let mut seen = Vec::new();
        let wb = create_project_reporting(
            &sub,
            "测试项目",
            &["产品策划".into(), "后端".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            None,
            None,
            |step| seen.push(step),
        )
        .unwrap();
        assert_eq!(seen, CREATE_STEP_ORDER.to_vec());
        assert!(sub.join(".hexagon/state.db").exists());
        drop(wb);
    }

    /// 票 14：失败停在该步，后面的步（尤其是打开项目）不报告，项目也没打开。
    #[test]
    fn create_failure_stops_before_later_steps() {
        let d = tempfile::tempdir().unwrap();

        // 脏树不再停（ADR 0060）：五步都报告，未提交改动还在，且没有被这次创建提交。
        git::init(d.path(), "main").unwrap();
        std::fs::write(d.path().join("README.md"), "x").unwrap();
        git::commit_all(d.path(), "init").unwrap();
        std::fs::write(d.path().join("README.md"), "dirty").unwrap();
        let mut seen = Vec::new();
        create_project_reporting(
            d.path(),
            "仓",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            false,
            &store_with_key(),
            &doc_with_provider(),
            None,
            None,
            |step| seen.push(step),
        )
        .unwrap();
        assert_eq!(seen, CREATE_STEP_ORDER.to_vec());
        assert_eq!(
            std::fs::read_to_string(d.path().join("README.md")).unwrap(),
            "dirty"
        );
        assert!(git::is_dirty(d.path()));

        // 无 git 且未确认：同样停在 git，不打开。
        // 单独的临时目录——上一段的仓库是父目录，子目录会被 rev-parse 认成仓库。
        let bare_root = tempfile::tempdir().unwrap();
        let bare = bare_root.path().join("bare");
        std::fs::create_dir(&bare).unwrap();
        seen.clear();
        let err = match create_project_reporting(
            &bare,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            false,
            &store_with_key(),
            &doc_with_provider(),
            None,
            None,
            |step| seen.push(step),
        ) {
            Ok(_) => panic!("no git should stop"),
            Err(e) => e,
        };
        assert!(matches!(err, SetupError::NoGit(_)));
        assert_eq!(seen, vec![CreateStep::CheckDir]);

        // 缺密钥：目录和 git 已过，角色落了库但复查失败。打开不报告，库被收回。
        // 不放进上面的脏仓库里，否则 git 闸会先停。
        let fresh = tempfile::tempdir().unwrap();
        let sub = fresh.path().join("nokey");
        seen.clear();
        let err = match create_project_reporting(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &MemoryStore::default(),
            &crate::provider_config::ProviderDoc::default(),
            None,
            None,
            |step| seen.push(step),
        ) {
            Ok(_) => panic!("missing keys should stop"),
            Err(e) => e,
        };
        assert!(matches!(err, SetupError::MissingKeys(_)));
        assert_eq!(
            seen,
            vec![
                CreateStep::CheckDir,
                CreateStep::Git,
                CreateStep::PersistRoles,
            ]
        );
        assert!(!seen.contains(&CreateStep::RecheckKeys));
        assert!(!seen.contains(&CreateStep::OpenProject));
        assert!(
            !sub.join(".hexagon/state.db").exists(),
            "missing keys must not leave a project behind"
        );
    }

    #[test]
    fn agents_md_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        write_agents_md(d.path(), &agents_md_draft("x")).unwrap();
        assert!(matches!(
            write_agents_md(d.path(), "new"),
            Err(SetupError::AgentsMdExists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(d.path().join("AGENTS.md")).unwrap(),
            agents_md_draft("x")
        );
        assert_eq!(
            inspect_dir(d.path()).instructions.as_deref(),
            Some("AGENTS.md")
        );
    }

    /// ADR 0069：向导不传档位 → 列默认 L4。传任何档都不建项目。
    #[test]
    fn create_project_rejects_autonomy_gear() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        let wb = create_project(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            None,
        )
        .unwrap();
        assert_eq!(
            crate::autonomy::level(&wb.db, &wb.project_id).unwrap(),
            "L4"
        );
        drop(wb);

        // 行为变了：以前传 L0 会停在所选档。现在任何档都拒绝，而且不建目录。
        let chosen = d.path().join("q");
        let err = create_project(
            &chosen,
            "q",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            Some("L0"),
        );
        assert!(matches!(
            err,
            Err(SetupError::Autonomy(
                crate::autonomy::AutonomyError::GearsRemoved
            ))
        ));
        assert!(!chosen.join(".hexagon").exists());

        let bad = d.path().join("bad");
        let err = create_project(
            &bad,
            "bad",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            Some("L9"),
        );
        assert!(matches!(
            err,
            Err(SetupError::Autonomy(
                crate::autonomy::AutonomyError::GearsRemoved
            ))
        ));
        assert!(!bad.join(".hexagon").exists());
    }
    #[test]
    fn claude_md_blocks_agents_md_and_is_left_untouched() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("CLAUDE.md"), "keep-me").unwrap();
        assert!(matches!(
            write_agents_md(d.path(), "# Demo\n"),
            Err(SetupError::AgentsMdExists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(d.path().join("CLAUDE.md")).unwrap(),
            "keep-me"
        );
        assert!(!d.path().join("AGENTS.md").exists());
        assert_eq!(
            inspect_dir(d.path()).instructions.as_deref(),
            Some("CLAUDE.md")
        );
    }

    /// 票 16：优化只回草稿。目录参数故意不存在——写盘只能是确认后的 write_agents_md。
    #[test]
    fn one_sentence_optimizes_without_writing_until_confirm() {
        let sentence = "一个本地待办清单";
        let scripted = "# Demo\n\n## 做什么\n一个本地待办清单。\n\n## Commands\n- Build:\n- Test:\n- Check:\n\n## Layout\n- 未知\n\n## Conventions\n-\n";
        let provider =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response(scripted)]);
        let dir = tempfile::tempdir().unwrap();
        let draft = optimize_agents_md("Demo", sentence, &[], &provider).unwrap();
        assert_eq!(draft, scripted);
        assert!(!dir.path().join("AGENTS.md").exists());
        assert!(!dir.path().join("CLAUDE.md").exists());

        let calls = provider.recorded();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].model_slot, "default");
        assert!(calls[0].tools.is_empty());
        assert_eq!(calls[0].messages.len(), 2);
        let system = block_text(&calls[0].messages[0]);
        let user = block_text(&calls[0].messages[1]);
        assert_eq!(system, optimize_system_prompt("Demo"));
        assert_eq!(AGENTS_MD_OPTIMIZE_PROMPT, ADR_0067_OPTIMIZE_PROMPT);
        assert!(
            system.contains("## Tech stack"),
            "2026-09-25 骨架补意向技术选型"
        );
        assert!(system.starts_with("# Demo\n") || system.contains("\n# Demo\n"));
        // prompt-engineering 票 09：占位符随英文化改名（{项目名} → {project_name}），新增 {language}。
        assert!(!system.contains("{project_name}") && !system.contains("{language}"));
        assert!(
            system.contains("Write the prose in English"),
            "测试构建未设界面语言 → 英文"
        );
        assert!(!system.contains(sentence));
        assert_eq!(user, sentence);
        assert!(scripted.contains("- Build:\n- Test:\n- Check:"));
        assert!(scripted.contains("未知"));
        assert!(!scripted.contains("npm") && !scripted.contains("cargo"));

        write_agents_md(dir.path(), &draft).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
            scripted
        );
        let md_names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".md"))
            .collect();
        assert_eq!(md_names, vec!["AGENTS.md".to_string()]);
    }

    /// prompt-engineering 票 09：向导提示词英文，草稿语言跟随界面语言。
    /// 回归：角色职责起草曾写死「起草一段中文职责」，英文界面也出中文。
    #[test]
    fn wizard_drafts_follow_interface_language() {
        crate::uilang::set_test_language(Some("ja"));
        let p = crate::provider::ScriptedProvider::new(vec![
            crate::turn::text_response("draft"),
            crate::turn::text_response("duty"),
        ]);
        optimize_agents_md("Demo", "a todo list", &[], &p).unwrap();
        draft_role_duty("后端开发", "", &p).unwrap();
        crate::uilang::set_test_language(None);
        let calls = p.recorded();
        assert!(block_text(&calls[0].messages[0]).contains("Write the prose in Japanese"));
        let duty = block_text(&calls[1].messages[0]);
        assert!(
            duty.contains("in Japanese") && duty.contains("\"后端开发\""),
            "{duty}"
        );
        assert!(!duty.contains("中文"));
    }

    #[test]
    fn blank_sentence_does_not_call_the_model() {
        let provider = crate::provider::ScriptedProvider::new(vec![]);
        let err = optimize_agents_md("Demo", "  \n", &[], &provider).unwrap_err();
        assert!(matches!(err, SetupError::EmptyBrief));
        assert!(provider.recorded().is_empty());
    }

    #[test]
    fn empty_model_reply_is_not_a_draft() {
        let provider =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response(" \n\t")]);
        let err = optimize_agents_md("Demo", "一句", &[], &provider).unwrap_err();
        assert!(matches!(err, SetupError::EmptyDraft));
    }

    /// 2026-09-25 答问优化流：答案拼进用户消息，系统提示词照旧。
    #[test]
    fn qa_answers_ride_the_user_message() {
        let provider =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response("draft")]);
        let qa = vec![
            BriefQA {
                question: "技术栈?".into(),
                answer: "React + Node".into(),
            },
            BriefQA {
                question: "服务对象?".into(),
                answer: "家庭用户".into(),
            },
        ];
        optimize_agents_md("Demo", "一个菜谱应用", &qa, &provider).unwrap();
        let calls = provider.recorded();
        let user = block_text(&calls[0].messages[1]);
        assert!(user.starts_with("一个菜谱应用"));
        assert!(user.contains("Q: 技术栈?") && user.contains("A: React + Node"));
        assert!(user.contains("Q: 服务对象?") && user.contains("A: 家庭用户"));
        // 空答案 = 旧形态（不追加答问块）
        let provider2 =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response("draft")]);
        optimize_agents_md("Demo", "一句话", &[], &provider2).unwrap();
        assert_eq!(block_text(&provider2.recorded()[0].messages[1]), "一句话");
    }

    /// 答问起草：系统走项目说明槽提示词，解析剥围栏、逐题打捞。
    /// 题数不设上限（owner 2026-09-25：封顶曾漏掉技术选型题），选项仍封顶 4。
    #[test]
    fn brief_questions_parse_uncapped_questions() {
        let scripted = "```json\n[\n  {\"question\":\"技术栈?\",\"options\":[\"React\",\"Python\",\"暂不指定\",\"第四项\",\"第五项\"]},\n  {\"question\":\"\",\"options\":[\"x\"]},\n  {\"question\":\"给谁用?\",\"options\":[]},\n  {\"question\":\"硬约束?\",\"options\":[\"离线优先\"]},\n  {\"question\":\"集成?\",\"options\":[\"无\",\"日历\"]},\n  {\"question\":\"第五题\",\"options\":[\"a\"]}\n]\n```";
        let provider =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response(scripted)]);
        let qs = brief_questions("Demo", "一个菜谱应用", &provider).unwrap();
        // 空题干/空选项被跳过；有效题全收（4 题不再截断）；选项封顶 4
        assert_eq!(qs.len(), 4);
        assert_eq!(qs[0].question, "技术栈?");
        assert_eq!(qs[0].options.len(), 4);
        assert_eq!(qs[2].question, "集成?");
        assert_eq!(qs[3].question, "第五题");
        let calls = provider.recorded();
        assert_eq!(calls[0].model_slot, crate::provider_config::BRIEF_SLOT);
        let system = block_text(&calls[0].messages[0]);
        assert!(system.contains("\"Demo\"") && system.contains("in English"));
    }

    #[test]
    fn brief_questions_empty_array_is_no_questions() {
        let provider =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response("[]")]);
        let qs = brief_questions("Demo", "一个菜谱应用", &provider).unwrap();
        assert!(qs.is_empty());
    }

    #[test]
    fn brief_questions_garbage_is_an_error_not_a_question() {
        let provider = crate::provider::ScriptedProvider::new(vec![
            crate::turn::text_response("sorry, I cannot"),
            crate::turn::text_response("[{\"q\":\"wrong shape\"}]"),
        ]);
        assert!(matches!(
            brief_questions("Demo", "一句", &provider).unwrap_err(),
            SetupError::BadQuestions(_)
        ));
        // 形态不对的题项（无 options）打捞为零也报
        assert!(matches!(
            brief_questions("Demo", "一句", &provider).unwrap_err(),
            SetupError::BadQuestions(_)
        ));
    }

    /// 角色种子：职责段落按项目说明批量起草；只收入参名、先到先得。
    /// ADR 0075：模型若仍带 globs 一律丢弃——新项目无目录结构，
    /// 虚构归属比空更糟（会在真实写入上误触发越权裁决）。
    #[test]
    fn role_seeds_follow_brief_and_drop_stray_names() {
        let roles = vec![
            RoleDef {
                name: "后端".into(),
                duty: "服务端实现".into(),
                reviewer: None,
                model_slot: "chat".into(),
                globs: vec![],
                skills: vec![],
            },
            RoleDef {
                name: "插画师".into(),
                duty: "出图".into(),
                reviewer: None,
                model_slot: "chat".into(),
                globs: vec![],
                skills: vec![],
            },
        ];
        let scripted = "[{\"name\":\"后端\",\"duty\":\"菜谱 API 与服务端实现。不写界面。\",\"globs\":[\"server/**\",\"src/api/**\"]},\
            {\"name\":\"黑客\",\"duty\":\"编造\",\"globs\":[\"etc/**\"]},\
            {\"name\":\"后端\",\"duty\":\"重复名\",\"globs\":[]},\
            {\"name\":\"插画师\",\"duty\":\"菜谱配图与版面图素。不碰代码。\",\"globs\":[]}]";
        let provider =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response(scripted)]);
        let seeds = draft_role_defs("一个菜谱应用（React + FastAPI）", &roles, &provider).unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].name, "后端");
        assert_eq!(seeds[0].duty, "菜谱 API 与服务端实现。不写界面。");
        // ADR 0075：脚本里模型仍产的 globs 被丢弃，不落到种子
        assert!(seeds[0].globs.is_empty());
        assert_eq!(seeds[1].name, "插画师");
        assert!(seeds[1].globs.is_empty());
        // 全部是足厚段落——不触发薄稿补跑
        assert_eq!(provider.recorded().len(), 1);
        let calls = provider.recorded();
        assert_eq!(calls[0].model_slot, crate::provider_config::ROLE_DRAFT_SLOT);
        let user = block_text(&calls[0].messages[1]);
        assert!(user.contains("菜谱应用") && user.contains("\"后端\""));
    }

    /// 薄稿修复（owner 实测 2026-09-25 的退化形态）：首项写足、余项一行。
    /// 薄稿与漏稿收成子集同提示词补跑一轮；薄换薄不换，漏稿补位薄也收。
    #[test]
    fn role_seeds_repair_thin_and_missing_once() {
        let mk = |name: &str| RoleDef {
            name: name.into(),
            duty: "d".into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec![],
            skills: vec![],
        };
        let roles = vec![mk("后端"), mk("插画师"), mk("运维")];
        // 首跑：插画师/运维薄稿；修复轮：插画师补足、运维仍薄
        let provider = crate::provider::ScriptedProvider::new(vec![
            crate::turn::text_response(
                "[{\"name\":\"后端\",\"duty\":\"菜谱 API 与服务端实现。不写界面。\"},\
                  {\"name\":\"插画师\",\"duty\":\"出图\"},{\"name\":\"运维\",\"duty\":\"部署\"}]",
            ),
            crate::turn::text_response(
                "[{\"name\":\"插画师\",\"duty\":\"产出界面图素与封面图。不写代码，不改仓库结构。\"},\
                  {\"name\":\"运维\",\"duty\":\"还是薄稿\"}]",
            ),
        ]);
        let seeds = draft_role_defs("一个菜谱应用", &roles, &provider).unwrap();
        assert_eq!(seeds.len(), 3);
        // 足厚原样；薄稿被修复稿替换；仍薄的留首稿（一轮为止，不循环）
        assert_eq!(seeds[0].duty, "菜谱 API 与服务端实现。不写界面。");
        assert_eq!(
            seeds[1].duty,
            "产出界面图素与封面图。不写代码，不改仓库结构。"
        );
        assert_eq!(seeds[2].duty, "部署");
        // 补跑只入参薄稿子集，后端不再发
        let calls = provider.recorded();
        assert_eq!(calls.len(), 2);
        let user2 = block_text(&calls[1].messages[1]);
        assert!(user2.contains("插画师") && user2.contains("运维") && !user2.contains("\"后端\""));
    }

    /// 漏稿也进补跑：首跑没返回的角色名在第二轮子集里。
    #[test]
    fn role_seeds_repair_covers_missing_names() {
        let mk = |name: &str| RoleDef {
            name: name.into(),
            duty: "d".into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec![],
            skills: vec![],
        };
        let roles = vec![mk("后端"), mk("插画师")];
        let provider = crate::provider::ScriptedProvider::new(vec![
            crate::turn::text_response(
                "[{\"name\":\"后端\",\"duty\":\"菜谱 API 与服务端实现。不写界面。\"}]",
            ),
            crate::turn::text_response(
                "[{\"name\":\"插画师\",\"duty\":\"产出界面图素。不写代码。\"}]",
            ),
        ]);
        let seeds = draft_role_defs("一个菜谱应用", &roles, &provider).unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[1].name, "插画师");
        assert_eq!(seeds[1].duty, "产出界面图素。不写代码。");
        assert_eq!(provider.recorded().len(), 2);
    }

    /// 余项照抄模板（或只在模板上套几个字）也算薄，要补跑。
    /// 否则卡面只有首项（产品策划）看起来变了。
    #[test]
    fn role_seeds_repair_template_echo() {
        let mk = |name: &str, duty: &str| RoleDef {
            name: name.into(),
            duty: duty.into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec![],
            skills: vec![],
        };
        let roles = vec![mk("产品策划", "写出规格"), mk("后端", "服务端实现")];
        let provider = crate::provider::ScriptedProvider::new(vec![
            crate::turn::text_response(
                "[{\"name\":\"产品策划\",\"duty\":\"为本菜谱应用写出范围与验收。不写实现，不改代码。\"},\
                  {\"name\":\"后端\",\"duty\":\"服务端实现。不写界面。\"}]",
            ),
            crate::turn::text_response(
                "[{\"name\":\"后端\",\"duty\":\"实现菜谱的数据与接口。不写界面，不改规格。\"}]",
            ),
        ]);
        let seeds = draft_role_defs("一个菜谱应用", &roles, &provider).unwrap();
        assert_eq!(seeds[1].duty, "实现菜谱的数据与接口。不写界面，不改规格。");
        assert_eq!(provider.recorded().len(), 2);
    }

    #[test]
    fn role_seeds_empty_input_never_calls_the_model() {
        let provider = crate::provider::ScriptedProvider::new(vec![]);
        assert!(draft_role_defs("一句", &[], &provider).unwrap().is_empty());
        assert!(provider.recorded().is_empty());
        let roles = vec![RoleDef {
            name: "后端".into(),
            duty: "d".into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec![],
            skills: vec![],
        }];
        assert!(matches!(
            draft_role_defs("  ", &roles, &provider).unwrap_err(),
            SetupError::EmptyBrief
        ));
        assert!(provider.recorded().is_empty());
    }

    #[test]
    fn role_seeds_garbage_is_an_error() {
        let provider = crate::provider::ScriptedProvider::new(vec![crate::turn::text_response(
            "no json here",
        )]);
        let roles = vec![RoleDef {
            name: "后端".into(),
            duty: "d".into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec![],
            skills: vec![],
        }];
        assert!(matches!(
            draft_role_defs("一句", &roles, &provider).unwrap_err(),
            SetupError::BadRoleDefs(_)
        ));
    }

    fn block_text(msg: &crate::provider::Message) -> String {
        msg.content
            .iter()
            .filter_map(|b| match b {
                crate::provider::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// ADR 0067 优化提示词原文。改常量而没改合同，这条会红。
    // ADR 0071 修订：合同原文换成英文版（ADR 0067 同步修订）。
    // 2026-09-25 修订：骨架补 Tech stack 节 + 声明答问块（owner 裁决：
    // 意向技术选型要进草稿；先问后答的优化流）。
    const ADR_0067_OPTIMIZE_PROMPT: &str = "You are drafting AGENTS.md at the repository root. This file holds the project-level constraints that activated agents read. It is not a skill; do not write process progress, secrets or a role roster into it.

The user gave a single sentence, possibly followed by answers to clarifying questions. Fill in the skeleton below so the file is as complete as the input honestly allows. For commands, tech stack or directories the user did not mention, leave them empty or write \"{unknown}\" — never invent them. Write the prose in {language}; keep the headings exactly as given.

# {project_name}

## Purpose
Two or three sentences. What the project does, who it serves, and — if the user's sentence implies boundaries — what it does not do.

## Tech stack
Intended languages, frameworks and key tools, one per line. Only what the user stated or picked in the answers; write \"{unknown}\" when nothing was stated.

## Commands
- Build:
- Test:
- Check:

## Layout
-

## Conventions
-

Output only the body of this file, with no preamble.";
}
