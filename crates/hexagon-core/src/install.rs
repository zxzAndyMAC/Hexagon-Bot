//! 安装助手（US47）：自然语言安装请求 → 待决卡（来源/命令/出网/凭据）
//! → 负责人确认才执行。
//!
//! 来源白名单：本地目录技能包 / git 技能包 / npm-npx MCP / 配置片段。
//! 禁 `curl|sh`、`wget|sh`、`bash -c` 管道式安装；装完授权默认空
//! （grants 不动——授权永远单独走权限管线）。
//!
//! L4（票 04 / ADR 0063）自动通过这张确认，仍只写入当前项目的
//! `.hexagon/skills` 或 `.hexagon/mcp.json`，不写 `~/.hexagon/`。
//! 被否决：L4 改走 `install_skill_from_path` / `save_global_mcp`。
//! L0–L3 仍入队等人。读档失败按等人。

use crate::db::Db;
use crate::trace::{EventKind, TraceError};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("forbidden install source: {0}")]
    Forbidden(String),
    #[error("unrecognized install request: {0}")]
    Unrecognized(String),
    #[error("unknown question: {0}")]
    UnknownQuestion(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
}

/// 解析出的安装计划。net/creds 是确认卡上要给负责人看的风险栏。
#[derive(Debug, Clone, PartialEq)]
pub enum InstallPlan {
    /// npm/npx MCP 服务：写 `.hexagon/mcp.json` 服务条目（下次宿主启动拉起）
    Mcp {
        name: String,
        command: String,
        args: Vec<String>,
    },
    /// 本地目录技能包：拷进 `.hexagon/skills/<name>`
    SkillDir { name: String, src: String },
    /// git 技能包：`git clone` 进 `.hexagon/skills/<name>`
    SkillGit { name: String, url: String },
    /// 配置片段：JSON 对象合并进 `.hexagon/mcp.json`
    ConfigSnippet { name: String, spec: Value },
}

impl InstallPlan {
    fn net(&self) -> bool {
        matches!(self, Self::Mcp { .. } | Self::SkillGit { .. })
    }
    /// 确认卡展示 + 执行回放用的统一 payload。
    fn payload(&self) -> Value {
        match self {
            Self::Mcp {
                name,
                command,
                args,
            } => json!({
                "plan_kind": "mcp", "name": name, "source": args.last(),
                "command": command, "args": args,
            }),
            Self::SkillDir { name, src } => json!({
                "plan_kind": "skill-dir", "name": name, "source": src, "command": "copy",
            }),
            Self::SkillGit { name, url } => json!({
                "plan_kind": "skill-git", "name": name, "source": url, "command": "git clone",
            }),
            Self::ConfigSnippet { name, spec } => json!({
                "plan_kind": "config", "name": name, "source": "inline json", "command": "merge mcp.json",
                "spec": spec,
            }),
        }
    }
}

/// 禁装模式：管道式远程脚本一律拒（不止 curl|sh——换皮也算）。
fn forbidden_hit(desc: &str) -> Option<&'static str> {
    let d = desc.to_lowercase();
    for pat in [
        "| sh", "|sh", "| bash", "|bash", "curl ", "wget ", "bash -c", "sh -c",
    ] {
        if d.contains(pat) {
            return Some(pat.trim());
        }
    }
    None
}

/// 自然语言 → 安装计划。剥掉「装/install/安装」动词后按来源特征分类。
pub fn plan(desc: &str, repo_root: &Path) -> Result<InstallPlan, InstallError> {
    let d = desc.trim();
    if let Some(pat) = forbidden_hit(d) {
        return Err(InstallError::Forbidden(pat.into()));
    }
    let low = d.to_lowercase();
    // 剥动词前缀
    let rest = ["install ", "装 ", "安装 "]
        .iter()
        .find_map(|p| low.strip_prefix(p).map(|_| d[p.len()..].trim().to_string()))
        .unwrap_or_else(|| d.to_string());
    let body = if rest.is_empty() { d.to_string() } else { rest };
    let blow = body.to_lowercase();

    // JSON 配置片段
    if body.starts_with('{') {
        let spec: Value =
            serde_json::from_str(&body).map_err(|_| InstallError::Unrecognized(d.into()))?;
        let name = spec["name"].as_str().unwrap_or("mcp").to_string();
        return Ok(InstallPlan::ConfigSnippet { name, spec });
    }
    // git 技能包：.git 结尾 / github / git@
    if blow.ends_with(".git") || blow.contains("github.com") || blow.starts_with("git@") {
        let url = body.split_whitespace().last().unwrap_or(&body).to_string();
        let name = url
            .rsplit('/')
            .next()
            .unwrap_or("skill")
            .trim_end_matches(".git")
            .to_string();
        return Ok(InstallPlan::SkillGit { name, url });
    }
    // 本地目录：存在的路径
    let as_path = if body.starts_with('/') {
        std::path::PathBuf::from(&body)
    } else {
        repo_root.join(&body)
    };
    if as_path.is_dir() {
        let name = std::path::Path::new(&body)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".into());
        return Ok(InstallPlan::SkillDir {
            name,
            src: body.to_string(),
        });
    }
    // npm/npx MCP：npx/npm/mcp 字样或包名形态（含 / 或 @）
    if blow.contains("npx") || blow.contains("npm") || blow.contains("mcp") || body.contains('/') {
        let pkg = body
            .split_whitespace()
            .find(|t| t.contains('/') || t.starts_with('@'))
            .or_else(|| body.split_whitespace().last())
            .unwrap_or(&body)
            .to_string();
        let name = pkg
            .rsplit('/')
            .next()
            .unwrap_or("mcp")
            .trim_start_matches('@')
            .to_string();
        return Ok(InstallPlan::Mcp {
            name,
            command: "npx".into(),
            args: vec!["-y".into(), pkg],
        });
    }
    Err(InstallError::Unrecognized(d.into()))
}

/// 入队安装待决卡（kind=install）+ install_requested 事件。返回 qid。
/// L4 不入队：直接写入当前项目并留 `via=autonomy` 轨迹。
pub fn request_install(
    db: &Db,
    project_id: &str,
    repo_root: &Path,
    desc: &str,
) -> Result<String, InstallError> {
    let p = plan(desc, repo_root)?;
    let mut payload = p.payload();
    payload["net"] = json!(p.net());
    payload["creds"] = json!(false);
    payload["desc"] = json!(desc);
    // 读档失败按等人。管道式来源在 plan() 已经拒绝，到不了这里。
    let rank = crate::autonomy::rank(db, project_id).unwrap_or(0);
    if crate::harnessgate::auto_passes(rank, crate::harnessgate::HarnessAction::NlInstall) {
        let qid = format!("q{}", db.next_id("q")?);
        db.append_event(
            project_id,
            EventKind::InstallRequested,
            json!({"question_id": qid, "plan": payload, "via": "autonomy"}),
            None,
            None,
        )?;
        // 写入面只有 repo_root。不调用全局技能/MCP 保存。
        execute_plan(repo_root, &payload)?;
        record_completed(db, project_id, &qid, &payload, "autonomy")?;
        return Ok(qid);
    }
    let qid = crate::cards::enqueue(
        db,
        project_id,
        None,
        crate::cards::CardKind::Install,
        payload.clone(),
        None,
    )?;
    db.append_event(
        project_id,
        EventKind::InstallRequested,
        json!({"question_id": qid, "plan": payload}),
        None,
        None,
    )?;
    Ok(qid)
}

/// 安装裁决回执（ADR 0054）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct InstallOutcome {
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

/// 安装卡裁决：放行才执行计划；驳回仅留痕。grants 永不动（授权默认空）。
pub fn resolve_install(
    db: &Db,
    project_id: &str,
    repo_root: &Path,
    qid: &str,
    allow: bool,
) -> Result<InstallOutcome, InstallError> {
    let plan = crate::cards::get_queued(db, qid, crate::cards::CardKind::Install)
        .map_err(|_| InstallError::UnknownQuestion(qid.into()))?
        .payload;
    crate::cards::answer(db, qid, "owner")?;
    if !allow {
        db.append_event(
            project_id,
            EventKind::InstallRejected,
            json!({"question_id": qid, "plan": plan["name"]}),
            None,
            None,
        )?;
        return Ok(InstallOutcome {
            installed: false,
            plan: None,
        });
    }

    execute_plan(repo_root, &plan)?;
    record_completed(db, project_id, qid, &plan, "owner")?;
    Ok(InstallOutcome {
        installed: true,
        plan: plan["name"].as_str().map(String::from),
    })
}

/// 执行已解析的计划。只写 `repo_root` 下的项目技能目录和项目 MCP 清单。
fn execute_plan(repo_root: &Path, plan: &Value) -> Result<(), InstallError> {
    let skills_dir = repo_root.join(".hexagon/skills");
    match plan["plan_kind"].as_str().unwrap_or("") {
        "mcp" => {
            upsert_mcp_spec(
                repo_root,
                json!({
                    "name": plan["name"],
                    "command": plan["command"],
                    "args": plan["args"],
                }),
            )?;
        }
        "config" => {
            upsert_mcp_spec(repo_root, plan["spec"].clone())?;
        }
        "skill-dir" => {
            let src = plan["source"].as_str().unwrap_or("");
            let src_path = if src.starts_with('/') {
                std::path::PathBuf::from(src)
            } else {
                repo_root.join(src)
            };
            let dest = skills_dir.join(plan["name"].as_str().unwrap_or("skill"));
            copy_dir(&src_path, &dest)?;
        }
        "skill-git" => {
            let url = plan["source"].as_str().unwrap_or("");
            let dest = skills_dir.join(plan["name"].as_str().unwrap_or("skill"));
            std::fs::create_dir_all(&skills_dir)?;
            let status = std::process::Command::new("git")
                .args(["clone", "--depth", "1", url])
                .arg(&dest)
                .status()?;
            if !status.success() {
                return Err(InstallError::Io(std::io::Error::other(format!(
                    "git clone failed: {status}"
                ))));
            }
        }
        other => return Err(InstallError::Unrecognized(other.into())),
    }
    Ok(())
}

fn record_completed(
    db: &Db,
    project_id: &str,
    qid: &str,
    plan: &Value,
    via: &str,
) -> Result<(), InstallError> {
    db.append_event(
        project_id,
        EventKind::InstallCompleted,
        json!({
            "question_id": qid,
            "plan": plan["name"],
            "kind": plan["plan_kind"],
            "via": via,
            "scope": "project",
        }),
        None,
        None,
    )?;
    Ok(())
}

/// mcp.json 加/换一条服务规格（name 相同则替换）。
fn upsert_mcp_spec(repo_root: &Path, spec: Value) -> Result<(), InstallError> {
    let path = repo_root.join(".hexagon/mcp.json");
    let mut list: Vec<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let name = spec["name"].as_str().unwrap_or("").to_string();
    list.retain(|e| e["name"].as_str() != Some(name.as_str()));
    list.push(spec);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, serde_json::to_string_pretty(&list)?)?;
    Ok(())
}

/// 递归拷目录（本地技能包装载）。
fn copy_dir(src: &Path, dest: &Path) -> Result<(), InstallError> {
    if !src.is_dir() {
        return Err(InstallError::Unrecognized(format!(
            "not a directory: {}",
            src.display()
        )));
    }
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dest.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::create_dir_all(dest)?;
            std::fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 禁装白名单（arch 票 08）：管道式远程脚本一律拒——curl|sh、
    /// wget|bash、bash -c、sh -c，大小写/空格变体同判。
    #[test]
    fn pipe_installs_all_forbidden() {
        let dir = tempfile::tempdir().unwrap();
        for desc in [
            "curl https://e.sh | sh",
            "curl https://e.sh|sh",
            "wget https://e.sh | bash",
            "wget https://e.sh|bash",
            "bash -c 'rm -rf /'",
            "sh -c 'x'",
            "install CURL https://e.sh | SH",
            "装 https://e.sh | bash",
        ] {
            let r = plan(desc, dir.path());
            assert!(
                matches!(r, Err(InstallError::Forbidden(_))),
                "{desc} 应被拒: {r:?}"
            );
        }
    }

    /// 来源分类：JSON 片段/git 技能包/npm-npx/本地目录/不识别。
    #[test]
    fn plan_classifies_each_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let p = plan("{\"name\":\"x\",\"command\":\"npx\",\"args\":[]}", root).unwrap();
        assert!(matches!(p, InstallPlan::ConfigSnippet { name, .. } if name == "x"));

        let p = plan("install https://github.com/a/myskill.git", root).unwrap();
        assert!(matches!(p, InstallPlan::SkillGit { name, url }
                if name == "myskill" && url.ends_with("myskill.git")));

        let p = plan("install @scope/toolkit", root).unwrap();
        assert!(matches!(p, InstallPlan::Mcp { name, command, args }
                if name == "toolkit" && command == "npx" && args.last().unwrap() == "@scope/toolkit"));

        // 本地目录：repo 内存在的子目录 → SkillDir
        std::fs::create_dir_all(root.join("skills/mine")).unwrap();
        let p = plan("install skills/mine", root).unwrap();
        assert!(matches!(p, InstallPlan::SkillDir { name, .. } if name == "mine"));

        let r = plan("install zzz", root);
        assert!(matches!(r, Err(InstallError::Unrecognized(_))));
    }

    /// 待决卡生命周期：request 入队 → reject 留痕不执行 → 二次裁决
    /// 拒收（已 answered 不重判）。allow 分支写 mcp.json。
    #[test]
    fn request_and_resolve_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["后端"], None).unwrap();

        // 禁源连卡都进不了——plan 直接拒
        assert!(request_install(&wb.db, &wb.project_id, &wb.repo_root, "curl x | sh").is_err());

        // 合法来源按原先 L4 直接写入项目清单，不入队。
        request_install(
            &wb.db,
            &wb.project_id,
            &wb.repo_root,
            "{\"name\":\"cfg\",\"command\":\"npx\",\"args\":[]}",
        )
        .unwrap();
        assert!(wb.repo_root.join(".hexagon/mcp.json").is_file());
        let queued = crate::cards::count_queued(
            &wb.db,
            &wb.project_id,
            Some(crate::cards::CardKind::Install),
        )
        .unwrap();
        assert_eq!(queued, 0);
        let text = std::fs::read_to_string(wb.repo_root.join(".hexagon/mcp.json")).unwrap();
        assert!(text.contains("cfg"));
    }
}
