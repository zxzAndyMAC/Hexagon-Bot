//! 头像提案（owner-followups-1015 票 20，规范 components D12「头像提案卡」补设计）。
//!
//! 画师为每个目标角色出一张图，汇成一份提案；负责人在时间线提案卡上逐格「换一张 / 跳过」，
//! 点「全部应用」才按角色写头像（走票 17 的 [`avatars::set_role_avatar`]）。
//!
//! **提案不是待决卡**（2026-10-17 负责人裁决）：不进 `pending_questions`、不进待决坞、
//! 托管不处理、不吃裁决键。被否决：做成待决卡复用裁决回路——托管会替负责人「批准」换头像，
//! ⌘↵ 会在负责人没看图时把整队头像换掉；头像是负责人的审美选择，不是授权问题。
//!
//! 落盘：`.hexagon/artist/<id>/proposal.json` + 每格 `<role-key>-<rev>.png`（纯文件，同头像
//! ADR 0052：读只要 repo_root，不占 wb 锁）。轨迹事实：每次外发前一条 `artist_image_request`，
//! 提案出炉一条 `artist_avatar_proposal`，应用一条 `artist_avatars_applied`。

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::artist::{ARTIST_CODE, ARTIST_ROLE};
use crate::db::Db;
use crate::image_gen::{self, ImageEndpoint};
use crate::trace::EventKind;

pub const IMAGE_REQUEST: &str = "artist_image_request";
pub const PROPOSAL: &str = "artist_avatar_proposal";
pub const APPLIED: &str = "artist_avatars_applied";

#[derive(Debug, thiserror::Error)]
pub enum ArtistError {
    #[error("没有这份头像提案: {0}")]
    ArtistProposalMissing(String),
    #[error("提案里没有这个角色的图: {0}")]
    ArtistRoleNotInProposal(String),
    #[error("出图失败: {0}")]
    ImageGenerationFailed(String),
    #[error(transparent)]
    Avatar(#[from] crate::avatars::AvatarError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Trace(#[from] crate::trace::TraceError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
}

/// 一格的落盘记录。`image` 是提案目录里的文件名；`failure` 是 [`image_gen::ImageFailure`] 码。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ItemRec {
    role: String,
    prompt: String,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    failure: Option<String>,
    #[serde(default)]
    revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    id: String,
    style: String,
    model: String,
    items: Vec<ItemRec>,
    #[serde(default)]
    applied: Vec<String>,
    /// 负责人在卡上「跳过」的角色（ticket 33 P3：原只在界面状态里，重载 / 重启就复原了）。
    #[serde(default)]
    skipped: Vec<String>,
}

/// 提案卡读面（IPC）。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ArtistProposal {
    pub id: String,
    pub style: String,
    pub model: String,
    pub items: Vec<ArtistProposalItem>,
    /// 已应用的角色（空 = 尚未应用）。
    pub applied: Vec<String>,
    /// 负责人跳过的角色（落盘，重载后照样跳过；ticket 33 P3）。
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ArtistProposalItem {
    pub role: String,
    /// 这张图用的出图提示词（卡上折叠展示）。
    pub prompt: String,
    /// PNG data URL；出图失败为 None。
    pub image: Option<String>,
    /// 失败分类码（`image_gen` 词表）；成功为 None。
    pub failure: Option<String>,
}

/// 谁在出图：工具回合（项目经理召唤，带激活）或负责人「换一张」（无 Agent）。
pub struct DrawCtx<'a> {
    pub db: &'a Db,
    pub root: &'a Path,
    pub project_id: &'a str,
    pub agent_id: Option<&'a str>,
    pub stage_run_id: Option<&'a str>,
}

/// 提案 id 进路径：只认核生成的 `ap<数字>`，挡穿目录。
fn check_id(id: &str) -> Result<(), ArtistError> {
    match id.strip_prefix("ap") {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Ok(()),
        _ => Err(ArtistError::ArtistProposalMissing(id.into())),
    }
}

fn dir(root: &Path, id: &str) -> PathBuf {
    root.join(".hexagon/artist").join(id)
}

fn file_key(role: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(role.as_bytes());
    d.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

fn read_manifest(root: &Path, id: &str) -> Result<Manifest, ArtistError> {
    check_id(id)?;
    let path = dir(root, id).join("proposal.json");
    if !path.exists() {
        return Err(ArtistError::ArtistProposalMissing(id.into()));
    }
    crate::db::validate_generic_file_access(&path)?;
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn write_manifest(root: &Path, m: &Manifest) -> Result<(), ArtistError> {
    let d = dir(root, &m.id);
    std::fs::create_dir_all(&d)?;
    let tmp = d.join("proposal.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(m)?)?;
    std::fs::rename(tmp, d.join("proposal.json"))?;
    Ok(())
}

fn image_url(root: &Path, id: &str, file: &str) -> Result<Option<String>, ArtistError> {
    use base64::Engine;
    let path = dir(root, id).join(file);
    if !path.exists() {
        return Ok(None);
    }
    crate::db::validate_generic_file_access(&path)?;
    let bytes = std::fs::read(path)?;
    Ok(Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )))
}

/// 角色的职责摘要（交给画师主对话模型的那一句，也记进出图轨迹的 `outbound`）。
pub fn duty_summary_of(db: &Db, project_id: &str, role: &str) -> String {
    let duty = crate::roles::role_def(db, project_id, role)
        .map(|d| d.duty)
        .unwrap_or_default();
    image_gen::duty_summary(&duty)
}

/// 一次出图：先写轨迹（外发的三样 + 提示词），再发，再记账（画师名下），再落图。
/// `prompt` 是画师主对话模型为该角色写的那条（票 28，`artist_prompt`），原样外发。
fn draw(
    cx: &DrawCtx,
    ep: &ImageEndpoint,
    proposal_id: &str,
    style: &str,
    role: &str,
    prompt: &str,
    revision: u32,
) -> Result<ItemRec, ArtistError> {
    let summary = duty_summary_of(cx.db, cx.project_id, role);
    cx.db.append_event(
        cx.project_id,
        EventKind::System,
        json!({"kind":IMAGE_REQUEST,"speaker":ARTIST_ROLE,"speaker_code":ARTIST_CODE,
            "proposal_id":proposal_id,"model":ep.model,
            "outbound":{"role":role,"duty_summary":summary,"style":style},"prompt":prompt}),
        cx.agent_id,
        cx.stage_run_id,
    )?;
    let started = std::time::Instant::now();
    let budget_hit = crate::usage::enforce_cap(cx.db, cx.project_id)?;
    let result = if budget_hit {
        Err(image_gen::ImageFailure {
            code: "budget_exhausted",
            status: None,
        })
    } else {
        let r = image_gen::generate(ep, prompt);
        crate::usage::record_artist_image(
            cx.db,
            cx.root,
            cx.project_id,
            cx.stage_run_id,
            &ep.model,
            r.is_ok(),
            r.as_ref().ok().and_then(|g| g.usage),
        )?;
        r
    };
    let mut rec = ItemRec {
        role: role.into(),
        prompt: prompt.into(),
        image: None,
        failure: None,
        revision,
    };
    match result {
        Ok(g) => {
            use base64::Engine;
            let b64 = g.data_url.trim_start_matches("data:image/png;base64,");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| ArtistError::ImageGenerationFailed(e.to_string()))?;
            let name = format!("{}-{revision}.png", file_key(role));
            let d = dir(cx.root, proposal_id);
            std::fs::create_dir_all(&d)?;
            std::fs::write(d.join(&name), bytes)?;
            rec.image = Some(name);
            note(cx, started, false, "image_generated");
        }
        Err(f) => {
            note(cx, started, true, f.code);
            rec.failure = Some(f.code.into());
        }
    }
    Ok(rec)
}

fn note(cx: &DrawCtx, started: std::time::Instant, warn: bool, code: &str) {
    crate::diag::note(
        crate::diag::CLASS_SLOT,
        warn,
        Some(cx.project_id),
        cx.agent_id,
        cx.stage_run_id,
        None,
        "artist_image",
        code,
        started,
    );
}

/// 新提案：按画师写好的 `(角色, 提示词)` 逐个出图，落盘 + 一条 `artist_avatar_proposal`。
pub fn create(
    cx: &DrawCtx,
    ep: &ImageEndpoint,
    style: &str,
    prompts: &[(String, String)],
) -> Result<ArtistProposal, ArtistError> {
    let id = format!("ap{}", cx.db.next_id("artist_proposal")?);
    let mut items = Vec::with_capacity(prompts.len());
    for (role, prompt) in prompts {
        items.push(draw(cx, ep, &id, style, role, prompt, 0)?);
    }
    let roles: Vec<&String> = prompts.iter().map(|(r, _)| r).collect();
    let m = Manifest {
        id: id.clone(),
        style: style.into(),
        model: ep.model.clone(),
        items,
        applied: vec![],
        skipped: vec![],
    };
    write_manifest(cx.root, &m)?;
    cx.db.append_event(
        cx.project_id,
        EventKind::System,
        json!({"kind":PROPOSAL,"speaker":ARTIST_ROLE,"speaker_code":ARTIST_CODE,
            "proposal_id":id,"style":style,"model":ep.model,"roles":roles}),
        cx.agent_id,
        cx.stage_run_id,
    )?;
    view(cx.root, &m)
}

fn view(root: &Path, m: &Manifest) -> Result<ArtistProposal, ArtistError> {
    let items = m
        .items
        .iter()
        .map(|it| {
            Ok(ArtistProposalItem {
                role: it.role.clone(),
                prompt: it.prompt.clone(),
                image: match &it.image {
                    Some(f) => image_url(root, &m.id, f)?,
                    None => None,
                },
                failure: it.failure.clone(),
            })
        })
        .collect::<Result<Vec<_>, ArtistError>>()?;
    Ok(ArtistProposal {
        id: m.id.clone(),
        style: m.style.clone(),
        model: m.model.clone(),
        items,
        applied: m.applied.clone(),
        skipped: m.skipped.clone(),
    })
}

/// 读面：提案卡渲染用。
pub fn load(root: &Path, id: &str) -> Result<ArtistProposal, ArtistError> {
    view(root, &read_manifest(root, id)?)
}

/// 「换一张」（负责人在提案卡上点）：同一条提示词再出一张，替换该格（旧图留在目录里不读）。
/// 不再找主对话模型改写提示词（票 28）：负责人要的是同一描述的另一张图，改写会让同一格
/// 前后画风漂移、也让卡上折叠的提示词与外发对不上。
/// 出图槽在召唤之后被改掉 / 解绑时同样精确检查，不回退 default。
pub fn regenerate(
    cx: &DrawCtx,
    access: &crate::image_gen::ImageAccess,
    id: &str,
    role: &str,
) -> Result<ArtistProposal, ArtistError> {
    let doc = crate::provider_config::load()
        .map_err(|e| ArtistError::ImageGenerationFailed(e.to_string()))?;
    if !matches!(
        crate::provider_config::image_generation_slot(&doc),
        crate::provider_config::ImageGenerationSlot::Configured { .. }
    ) {
        return Err(ArtistError::ImageGenerationFailed(
            "image slot not bound".into(),
        ));
    }
    let ep = access
        .endpoint(&doc)
        .map_err(|f| ArtistError::ImageGenerationFailed(f.code.into()))?;
    redraw(cx, &ep, id, role)
}

fn redraw(
    cx: &DrawCtx,
    ep: &ImageEndpoint,
    id: &str,
    role: &str,
) -> Result<ArtistProposal, ArtistError> {
    let mut m = read_manifest(cx.root, id)?;
    let idx = m
        .items
        .iter()
        .position(|it| it.role == role)
        .ok_or_else(|| ArtistError::ArtistRoleNotInProposal(role.into()))?;
    let next = m.items[idx].revision + 1;
    let style = m.style.clone();
    let prompt = m.items[idx].prompt.clone();
    let rec = draw(cx, ep, id, &style, role, &prompt, next)?;
    m.items[idx] = rec;
    m.model = ep.model.clone();
    write_manifest(cx.root, &m)?;
    view(cx.root, &m)
}

/// 「跳过 / 取消跳过」一格（负责人在提案卡上点）。ticket 33（2026-10-25 批次审查 P3，原票 #20 遗留
/// 「Skip state not persisted」）：跳过原来只是界面状态，重载时间线 / 重启后被跳过的格又回到「待应用」，
/// 负责人点「全部应用」会把明明跳过的头像也换上。现在写进提案 manifest，读面带回。不写轨迹（不是业务事实，
/// 只是负责人在卡上的勾选）；角色不在提案里拒。
pub fn set_skipped(
    root: &Path,
    id: &str,
    role: &str,
    skipped: bool,
) -> Result<ArtistProposal, ArtistError> {
    let mut m = read_manifest(root, id)?;
    if !m.items.iter().any(|it| it.role == role) {
        return Err(ArtistError::ArtistRoleNotInProposal(role.into()));
    }
    m.skipped.retain(|r| r != role);
    if skipped {
        m.skipped.push(role.to_string());
    }
    write_manifest(root, &m)?;
    view(root, &m)
}

/// 「全部应用」：负责人勾下的格（未跳过、有图）按角色写头像。任何一格没图或不在提案里
/// 整单拒，什么都不写（fail-closed：半套头像比一套都没换更难解释）。
pub fn apply(
    db: &Db,
    root: &Path,
    project_id: &str,
    id: &str,
    roles: &[String],
) -> Result<ArtistProposal, ArtistError> {
    let mut m = read_manifest(root, id)?;
    let mut picked = Vec::new();
    for role in roles {
        let it = m
            .items
            .iter()
            .find(|it| &it.role == role)
            .ok_or_else(|| ArtistError::ArtistRoleNotInProposal(role.clone()))?;
        let file = it
            .image
            .as_deref()
            .ok_or_else(|| ArtistError::ArtistRoleNotInProposal(role.clone()))?;
        let url = image_url(root, id, file)?
            .ok_or_else(|| ArtistError::ArtistRoleNotInProposal(role.clone()))?;
        picked.push((role.clone(), url));
    }
    for (role, url) in &picked {
        crate::avatars::set_role_avatar(root, role, url)?;
    }
    for (role, _) in &picked {
        if !m.applied.contains(role) {
            m.applied.push(role.clone());
        }
    }
    write_manifest(root, &m)?;
    db.append_event(
        project_id,
        EventKind::System,
        json!({"kind":APPLIED,"proposal_id":id,"roles":roles}),
        None,
        None,
    )?;
    view(root, &m)
}
