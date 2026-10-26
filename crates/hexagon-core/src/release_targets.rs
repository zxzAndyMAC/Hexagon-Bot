//! 发布目标（owner-followups-1015 #14，ADR 0093 / CONTEXT.md「发布目标」）：项目已记住的一条发布命令
//! 及其所需凭据。不是 git 远程（那是推送，见 push.rs）。
//!
//! - 只有负责人经设置·发布页登记 / 修改 / 删除（宿主 control 组命令）；Agent 没有写入口，
//!   托管不扩大这项授权（CONTEXT.md「托管」：新增发布目标不扩大授权）。执行见 `crate::release`（票 15）。
//!   聊天里 Agent 只能拟稿（`crate::release_chat`，票 16）：拟稿进「登记发布目标」卡，负责人亲手批准时
//!   才经本模块 [`save`] 落库——写入口仍只有负责人这一个。
//! - 持久化在 `release_targets` 表（迁移 0065）：名称、命令行、工作目录（相对项目根）、凭据名列表。
//! - 凭据值只进钥匙串 `release/<目标 id>/<凭据名>`（[`credential_key`]）。表、轨迹、诊断、错误文案
//!   只出现名字；读面只回「已存入 / 未设置」，存后不回显。
//! - 值不进命令行：登记或录入时，若已存的凭据值出现在名称 / 命令 / 工作目录里即拒绝
//!   （`release_secret_in_target`）——负责人把令牌直接粘进命令时，令牌会随命令进卡片与轨迹。
//!   代价模型：误拒只花一次改命令；漏放是一次明文入库。偏向拒绝。

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::trace::EventKind;

/// 名称上限（字符）：列表与卡片要能放下，也挡住整段粘贴。
pub const MAX_NAME_CHARS: usize = 64;
/// 单个目标声明的凭据名上限。
pub const MAX_CREDENTIALS: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    #[error("release target name is invalid: {0}")]
    ReleaseNameInvalid(String),
    #[error("release target name already used: {0}")]
    ReleaseNameTaken(String),
    #[error("release command is empty or spans several lines")]
    ReleaseCommandInvalid,
    #[error("release working directory must stay inside the project: {0}")]
    ReleaseCwdOutside(String),
    #[error("credential name must look like an environment variable (A-Z, 0-9, _): {0}")]
    ReleaseCredentialNameInvalid(String),
    #[error("too many credentials for one release target (max {MAX_CREDENTIALS})")]
    ReleaseTooManyCredentials,
    #[error("unknown release target: {0}")]
    ReleaseTargetUnknown(String),
    #[error("credential {credential} is not declared by release target {target}")]
    ReleaseCredentialUndeclared { target: String, credential: String },
    #[error("credential value is empty")]
    ReleaseSecretEmpty,
    /// 文案不带任何值，也不点名是哪一项——名字本身就足够负责人去改。
    #[error(
        "a stored credential value appears in the release target; keep secrets out of the command"
    )]
    ReleaseSecretInTarget,
    /// 只有负责人能登记 / 修改 / 删除目标与凭据（owner-followups-1015 #15：托管可确认发布卡，但不能改登记）。
    #[error("only the owner can change release targets and their credentials")]
    ReleaseOwnerOnly,
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("trace: {0}")]
    Trace(#[from] crate::trace::TraceError),
    #[error("credential: {0}")]
    Cred(#[from] crate::credentials::CredError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// 一条凭据的读面：只有名字和「已存入」，值永不出钥匙串。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReleaseCredential {
    pub name: String,
    pub set: bool,
}

/// 发布目标读面（设置·发布页列表 | 详情）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReleaseTarget {
    pub id: String,
    pub name: String,
    /// 命令行原文（单行），执行时在 `cwd` 下运行（票 15）。
    pub command: String,
    /// 相对项目根；空串 = 项目根。
    pub cwd: String,
    pub credentials: Vec<ReleaseCredential>,
}

/// 登记 / 修改的入参：`id` 为空 = 新建。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReleaseTargetDraft {
    #[ts(optional)]
    pub id: Option<String>,
    pub name: String,
    pub command: String,
    pub cwd: String,
    pub credentials: Vec<String>,
}

/// 钥匙串条目名（credentials.rs 命名约定）。按目标 id 而非目标名：改名不搬钥匙串条目
/// （搬一半失败会丢唯一一份凭据，同 push::credential 的取舍），id 随机所以跨项目不撞名
/// （项目 id 是常量 `crate::PROJECT_ID`，钥匙串全机共享）。
pub fn credential_key(target_id: &str, credential: &str) -> String {
    format!("release/{target_id}/{credential}")
}

/// 名称判定：去首尾空白后 1..=64 字符，无控制字符。
pub fn normalize_name(raw: &str) -> Result<String, ReleaseError> {
    let name = raw.trim();
    let n = name.chars().count();
    if n == 0 || n > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(ReleaseError::ReleaseNameInvalid(
            name.chars().take(MAX_NAME_CHARS).collect(),
        ));
    }
    Ok(name.to_string())
}

/// 命令行判定：非空、单行、无控制字符（tab 除外）。
pub fn normalize_command(raw: &str) -> Result<String, ReleaseError> {
    let cmd = raw.trim();
    if cmd.is_empty() || cmd.chars().any(|c| c.is_control() && c != '\t') {
        return Err(ReleaseError::ReleaseCommandInvalid);
    }
    Ok(cmd.to_string())
}

/// 工作目录判定：相对项目根，不得以绝对路径、盘符或 `..` 走出项目（fail-closed：
/// 误拒只花一次改路径，误放是一条在项目外运行的发布命令）。返回 `/` 连接的规范形，`""` = 项目根。
pub fn normalize_cwd(raw: &str) -> Result<String, ReleaseError> {
    let trimmed = raw.trim();
    if trimmed.chars().any(char::is_control) {
        return Err(ReleaseError::ReleaseCwdOutside(
            trimmed.escape_default().to_string(),
        ));
    }
    // Windows 风格分隔符一并按分隔符看——`..\\x` 不能借反斜杠绕过 `..` 判定。
    let unified = trimmed.replace('\\', "/");
    let mut parts = Vec::new();
    for c in Path::new(&unified).components() {
        match c {
            // 段首尾空白一并去掉（proptest 种子 `"A \\"`：`A /` 规范成 `A `，再判一次变 `A`，不幂等）。
            Component::Normal(s) => {
                let seg = s.to_string_lossy().trim().to_string();
                if seg == ".." {
                    return Err(ReleaseError::ReleaseCwdOutside(trimmed.to_string()));
                }
                if !seg.is_empty() && seg != "." {
                    parts.push(seg);
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ReleaseError::ReleaseCwdOutside(trimmed.to_string()));
            }
        }
    }
    // `C:foo` 在 Unix 上是 Normal 组件，挡掉盘符形，免得同一份配置换平台变成项目外。
    if parts
        .first()
        .is_some_and(|p| p.len() >= 2 && p.as_bytes()[1] == b':')
    {
        return Err(ReleaseError::ReleaseCwdOutside(trimmed.to_string()));
    }
    Ok(parts.join("/"))
}

/// 凭据名判定：环境变量形 `[A-Za-z_][A-Za-z0-9_]{0,63}`——执行时作为环境变量交给发布命令
/// （票 15），值不拼进命令行。
pub fn normalize_credential_name(raw: &str) -> Result<String, ReleaseError> {
    let name = raw.trim();
    let mut chars = name.chars();
    let ok = name.len() <= 64
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(ReleaseError::ReleaseCredentialNameInvalid(
            name.chars().take(64).collect(),
        ));
    }
    Ok(name.to_string())
}

pub(crate) fn normalize_credentials(raw: &[String]) -> Result<Vec<String>, ReleaseError> {
    let mut out: Vec<String> = Vec::new();
    for r in raw {
        if r.trim().is_empty() {
            continue;
        }
        let n = normalize_credential_name(r)?;
        if !out.contains(&n) {
            out.push(n);
        }
    }
    if out.len() > MAX_CREDENTIALS {
        return Err(ReleaseError::ReleaseTooManyCredentials);
    }
    Ok(out)
}

/// 登记行原样（不碰钥匙串）：执行发布（`crate::release`，票 15）按它比对卡面、取命令与凭据名。
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) credentials: Vec<String>,
}

fn rows(db: &Db, project_id: &str, id: Option<&str>) -> Result<Vec<Row>, ReleaseError> {
    let mut st = db.conn().prepare(
        "SELECT id, name, command, cwd, credentials FROM release_targets
         WHERE project_id=?1 AND (?2 IS NULL OR id=?2) ORDER BY created_at, rowid",
    )?;
    let raw = st
        .query_map(rusqlite::params![project_id, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    raw.into_iter()
        .map(|(id, name, command, cwd, creds)| {
            Ok(Row {
                id,
                name,
                command,
                cwd,
                credentials: serde_json::from_str(&creds)?,
            })
        })
        .collect()
}

/// 按 id 或登记名（去首尾空白后精确相等）找目标；聊天发起发布（票 16）只认这两种写法，
/// 不做模糊匹配——「差不多像」的名字选错目标就是一次发错的不可逆发布。
pub(crate) fn find(db: &Db, project_id: &str, key: &str) -> Result<Option<Row>, ReleaseError> {
    let key = key.trim();
    Ok(rows(db, project_id, None)?
        .into_iter()
        .find(|r| r.id == key || r.name == key))
}

/// 全部登记的名字（登记顺序）：拒绝时告诉模型能选哪些。
pub(crate) fn names(db: &Db, project_id: &str) -> Result<Vec<String>, ReleaseError> {
    Ok(rows(db, project_id, None)?
        .into_iter()
        .map(|r| r.name)
        .collect())
}

/// 已存的全部发布凭据值（各目标、各凭据名）。只给批准拟稿时的泄漏比对用，值不出调用方。
pub(crate) fn stored_secrets(
    db: &Db,
    project_id: &str,
    store: &dyn CredentialStore,
) -> Result<Vec<String>, ReleaseError> {
    let mut out = Vec::new();
    for r in rows(db, project_id, None)? {
        for name in &r.credentials {
            if let Some(v) = store.get(&credential_key(&r.id, name))? {
                if !v.is_empty() {
                    out.push(v);
                }
            }
        }
    }
    Ok(out)
}

pub(crate) fn row(db: &Db, project_id: &str, id: &str) -> Result<Row, ReleaseError> {
    rows(db, project_id, Some(id))?
        .pop()
        .ok_or_else(|| ReleaseError::ReleaseTargetUnknown(id.to_string()))
}

fn view(row: Row, store: &dyn CredentialStore) -> Result<ReleaseTarget, ReleaseError> {
    let credentials = row
        .credentials
        .iter()
        .map(|name| {
            // 只问「有没有」，值在这一行内丢弃。
            let set = store.get(&credential_key(&row.id, name))?.is_some();
            Ok(ReleaseCredential {
                name: name.clone(),
                set,
            })
        })
        .collect::<Result<Vec<_>, ReleaseError>>()?;
    Ok(ReleaseTarget {
        id: row.id,
        name: row.name,
        command: row.command,
        cwd: row.cwd,
        credentials,
    })
}

/// 列出本项目全部发布目标（登记顺序）。
pub fn list(
    db: &Db,
    project_id: &str,
    store: &dyn CredentialStore,
) -> Result<Vec<ReleaseTarget>, ReleaseError> {
    rows(db, project_id, None)?
        .into_iter()
        .map(|r| view(r, store))
        .collect()
}

/// 读一个发布目标。
pub fn get(
    db: &Db,
    project_id: &str,
    store: &dyn CredentialStore,
    id: &str,
) -> Result<ReleaseTarget, ReleaseError> {
    view(row(db, project_id, id)?, store)
}

fn new_id() -> Result<String, ReleaseError> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes)
        .map_err(|e| crate::credentials::CredError::Store(format!("random id: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// 已存凭据值是否出现在目标的可见字段里（名称 / 命令 / 工作目录）。
fn secret_visible(
    store: &dyn CredentialStore,
    target_id: &str,
    credentials: &[String],
    fields: &[&str],
    extra: Option<&str>,
) -> Result<bool, ReleaseError> {
    let hit = |secret: &str| !secret.is_empty() && fields.iter().any(|f| f.contains(secret));
    if extra.is_some_and(hit) {
        return Ok(true);
    }
    for name in credentials {
        if let Some(secret) = store.get(&credential_key(target_id, name))? {
            if hit(&secret) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn reject(err: ReleaseError, branch: &str, started: std::time::Instant) -> ReleaseError {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        branch,
        &crate::errcode::variant_code(&err),
        started,
    );
    err
}

/// 登记（`draft.id` 为空）或修改一个发布目标。修改时从列表里拿掉的凭据名，钥匙串条目先删再写表——
/// 删失败则表不变，不留下界面看不见的孤儿秘密。
pub fn save(
    db: &Db,
    project_id: &str,
    store: &dyn CredentialStore,
    draft: &ReleaseTargetDraft,
) -> Result<ReleaseTarget, ReleaseError> {
    let started = std::time::Instant::now();
    let checked = (|| {
        let name = normalize_name(&draft.name)?;
        let command = normalize_command(&draft.command)?;
        let cwd = normalize_cwd(&draft.cwd)?;
        let credentials = normalize_credentials(&draft.credentials)?;
        Ok::<_, ReleaseError>((name, command, cwd, credentials))
    })();
    let (name, command, cwd, credentials) =
        checked.map_err(|e| reject(e, "release_target_save", started))?;
    let existing = match draft.id.as_deref() {
        Some(id) => Some(row(db, project_id, id)?),
        None => None,
    };
    let taken: bool = db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM release_targets WHERE project_id=?1 AND name=?2 AND id<>?3)",
        rusqlite::params![project_id, name, draft.id.as_deref().unwrap_or("")],
        |r| r.get(0),
    )?;
    if taken {
        return Err(reject(
            ReleaseError::ReleaseNameTaken(name),
            "release_target_save",
            started,
        ));
    }
    let id = match &existing {
        Some(r) => r.id.clone(),
        None => new_id()?,
    };
    if existing.is_some()
        && secret_visible(store, &id, &credentials, &[&name, &command, &cwd], None)?
    {
        return Err(reject(
            ReleaseError::ReleaseSecretInTarget,
            "release_target_save",
            started,
        ));
    }
    if let Some(old) = &existing {
        for dropped in old.credentials.iter().filter(|c| !credentials.contains(c)) {
            store.delete(&credential_key(&id, dropped))?;
        }
    }
    let creds_json = serde_json::to_string(&credentials)?;
    if existing.is_some() {
        db.conn().execute(
            "UPDATE release_targets SET name=?3, command=?4, cwd=?5, credentials=?6,
             updated_at=datetime('now') WHERE project_id=?1 AND id=?2",
            rusqlite::params![project_id, id, name, command, cwd, creds_json],
        )?;
    } else {
        db.conn().execute(
            "INSERT INTO release_targets (id, project_id, name, command, cwd, credentials)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, project_id, name, command, cwd, creds_json],
        )?;
    }
    // 轨迹只记名字：命令行原文留在表里，事件里不重复（命令可能带私有路径，读面已有）。
    db.append_event(
        project_id,
        EventKind::System,
        json!({
            "kind": "release_target_changed",
            "action": if existing.is_some() { "updated" } else { "added" },
            "target_id": id,
            "name": name,
            "credentials": credentials,
            "by": "owner",
        }),
        None,
        None,
    )?;
    get(db, project_id, store, &id)
}

/// 删除一个发布目标：先删它在钥匙串里的全部凭据，再删登记。
pub fn remove(
    db: &Db,
    project_id: &str,
    store: &dyn CredentialStore,
    id: &str,
) -> Result<(), ReleaseError> {
    let target = row(db, project_id, id)?;
    for name in &target.credentials {
        store.delete(&credential_key(id, name))?;
    }
    db.conn().execute(
        "DELETE FROM release_targets WHERE project_id=?1 AND id=?2",
        rusqlite::params![project_id, id],
    )?;
    db.append_event(
        project_id,
        EventKind::System,
        json!({
            "kind": "release_target_changed",
            "action": "removed",
            "target_id": id,
            "name": target.name,
            "by": "owner",
        }),
        None,
        None,
    )?;
    Ok(())
}

/// 录入 / 替换一条凭据（密文进钥匙串，返回更新后的读面——仍只有「已存入」）。
pub fn set_credential(
    db: &Db,
    project_id: &str,
    store: &dyn CredentialStore,
    id: &str,
    credential: &str,
    secret: &str,
) -> Result<ReleaseTarget, ReleaseError> {
    let started = std::time::Instant::now();
    let target = row(db, project_id, id)?;
    let declared =
        declared(&target, credential).map_err(|e| reject(e, "release_credential_set", started))?;
    // 粘贴常带换行：首尾空白不算值的一部分。
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(reject(
            ReleaseError::ReleaseSecretEmpty,
            "release_credential_set",
            started,
        ));
    }
    if secret_visible(
        store,
        id,
        &[],
        &[&target.name, &target.command, &target.cwd],
        Some(secret),
    )? {
        return Err(reject(
            ReleaseError::ReleaseSecretInTarget,
            "release_credential_set",
            started,
        ));
    }
    let key = credential_key(id, &declared);
    let replaced = store.get(&key)?.is_some();
    store.set(&key, secret)?;
    db.append_event(
        project_id,
        EventKind::System,
        json!({
            "kind": "release_credential_changed",
            "action": if replaced { "replaced" } else { "set" },
            "target_id": id,
            "target": target.name,
            "credential": declared,
            "by": "owner",
        }),
        None,
        None,
    )?;
    get(db, project_id, store, id)
}

/// 移除一条凭据（钥匙串删除；名字仍留在目标的所需凭据列表里，显示「未设置」）。
pub fn remove_credential(
    db: &Db,
    project_id: &str,
    store: &dyn CredentialStore,
    id: &str,
    credential: &str,
) -> Result<ReleaseTarget, ReleaseError> {
    let started = std::time::Instant::now();
    let target = row(db, project_id, id)?;
    let declared = declared(&target, credential)
        .map_err(|e| reject(e, "release_credential_remove", started))?;
    store.delete(&credential_key(id, &declared))?;
    db.append_event(
        project_id,
        EventKind::System,
        json!({
            "kind": "release_credential_changed",
            "action": "removed",
            "target_id": id,
            "target": target.name,
            "credential": declared,
            "by": "owner",
        }),
        None,
        None,
    )?;
    get(db, project_id, store, id)
}

fn declared(target: &Row, credential: &str) -> Result<String, ReleaseError> {
    let name = credential.trim();
    if target.credentials.iter().any(|c| c == name) {
        Ok(name.to_string())
    } else {
        Err(ReleaseError::ReleaseCredentialUndeclared {
            target: target.name.clone(),
            credential: name.chars().take(64).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn cwd_rules() {
        assert_eq!(normalize_cwd("").unwrap(), "");
        assert_eq!(normalize_cwd(" . ").unwrap(), "");
        assert_eq!(normalize_cwd("./packages/web/").unwrap(), "packages/web");
        for bad in [
            "/etc",
            "../x",
            "a/../../b",
            "a/..",
            "..\\x",
            "C:\\x",
            "c:foo",
            "a\nb",
        ] {
            assert!(
                matches!(normalize_cwd(bad), Err(ReleaseError::ReleaseCwdOutside(_))),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn credential_name_rules() {
        assert_eq!(
            normalize_credential_name(" NPM_TOKEN ").unwrap(),
            "NPM_TOKEN"
        );
        assert!(normalize_credential_name("_x1").is_ok());
        for bad in ["", "1X", "A-B", "A B", "release/x", "ÄB"] {
            assert!(normalize_credential_name(bad).is_err(), "{bad:?}");
        }
    }

    proptest! {
        /// 根约束不变量：任何被接受的工作目录拼到项目根下都留在项目根内（无 `..`、非绝对），
        /// 且规范形再判一次不变（幂等）。
        #[test]
        fn accepted_cwd_stays_inside_project(raw in "[a-zA-Z0-9_./\\\\: -]{0,24}") {
            if let Ok(cwd) = normalize_cwd(&raw) {
                let root = Path::new("/proj");
                let joined = root.join(&cwd);
                prop_assert!(joined.starts_with(root));
                prop_assert!(!Path::new(&cwd).is_absolute());
                prop_assert!(Path::new(&cwd).components().all(|c| matches!(c, Component::Normal(_))));
                prop_assert_eq!(normalize_cwd(&cwd).unwrap(), cwd);
            }
        }

        /// 凭据名接受面只有环境变量形，钥匙串名因此恒为三段 `release/<id>/<名>`。
        #[test]
        fn accepted_credential_names_keep_keychain_names_three_part(raw in "\\PC{0,12}") {
            if let Ok(n) = normalize_credential_name(&raw) {
                prop_assert_eq!(credential_key("abc", &n).split('/').count(), 3);
            }
        }
    }
}
