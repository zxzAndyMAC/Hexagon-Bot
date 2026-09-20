//! 远程发布（票 16）：显式指令 + 双闸，无自动路径。
//!
//! - **不进工具注册表**——Agent 拿不到 `remote_publish` 工具，任何自治档都
//!   无法触发；唯一入口是负责人的 API 指令（确认卡）。
//! - 双闸：发布确认卡（kind='publish' 必问，写明远端与不可逆提示，永不记忆）
//!   + 钥匙串发布凭据 `publish/<remote>`，缺凭据即明确失败。
//! - 执行走 git 管道短命令；确认人、时间、目标、结果全落轨迹。

use serde_json::json;

use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::trace::EventKind;

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("trace: {0}")]
    Trace(#[from] crate::trace::TraceError),
    #[error("credential: {0}")]
    Cred(#[from] crate::credentials::CredError),
    #[error("git: {0}")]
    Git(#[from] crate::git::GitError),
    #[error("unknown publish question: {0}")]
    UnknownQuestion(String),
}

/// 发起发布：只入队确认卡，不执行任何东西。
pub fn request(db: &Db, project_id: &str, remote: &str) -> Result<String, PublishError> {
    let baseline = crate::git::baseline_branch(std::path::Path::new(&db.conn().query_row(
        "SELECT dir FROM projects WHERE id=?1",
        [project_id],
        |r| r.get::<_, String>(0),
    )?))
    .unwrap_or_else(|_| "main".into());
    let qid = crate::cards::enqueue(
        db,
        project_id,
        None,
        crate::cards::CardKind::Publish,
        json!({
            "remote": remote,
            "baseline": baseline,
            "warning": "远程发布不可逆：代码与产物将推送到项目外部",
        }),
        None,
    )?;
    db.append_event(
        project_id,
        EventKind::PublishRequested,
        json!({"remote": remote, "baseline": baseline, "question_id": qid}),
        None,
        None,
    )?;
    log::info!("publish requested: remote={remote} qid={qid}");
    Ok(qid)
}

/// 发布回执（ADR 0054）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PublishOutcome {
    pub remote: String,
    pub baseline: String,
    pub output: String,
}

/// 确认发布：凭据闸 + git push。确认事实先落轨迹，失败单独落 PublishFailed。
pub fn confirm(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
) -> Result<PublishOutcome, PublishError> {
    // 卡表读写归 cards.rs（arch-review 票 04）
    let p = crate::cards::get_queued(db, qid, crate::cards::CardKind::Publish)
        .map_err(|_| PublishError::UnknownQuestion(qid.into()))?
        .payload;
    let remote = p["remote"].as_str().unwrap_or("origin").to_string();
    let baseline = p["baseline"].as_str().unwrap_or("main").to_string();

    crate::cards::answer(db, qid, "owner")?;

    let dir: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
                r.get(0)
            })?;
    let repo = std::path::Path::new(&dir);
    let result = (|| -> Result<PublishOutcome, PublishError> {
        // 凭据闸：publish/<remote> 必须存在于钥匙串（值只验证存在，不进命令行/日志）
        let cred_name = format!("publish/{remote}");
        store
            .get(&cred_name)?
            .ok_or(crate::credentials::CredError::Missing(cred_name))?;
        let out = crate::git::run(repo, &["push", "-u", &remote, &baseline])?;
        Ok(PublishOutcome {
            remote: remote.clone(),
            baseline: baseline.clone(),
            output: out,
        })
    })();

    match result {
        Ok(v) => {
            db.append_event(
                project_id,
                EventKind::PublishConfirmed,
                json!({"remote": remote, "baseline": baseline, "question_id": qid}),
                None,
                None,
            )?;
            log::info!("publish confirmed: {remote} {baseline}");
            Ok(v)
        }
        Err(e) => {
            db.append_event(
                project_id,
                EventKind::PublishFailed,
                json!({"remote": remote, "question_id": qid, "error": e.to_string()}),
                None,
                None,
            )?;
            log::warn!("publish failed: {e}");
            Err(e)
        }
    }
}

/// 拒绝发布：标记已答 + 轨迹。
pub fn reject(db: &Db, project_id: &str, qid: &str) -> Result<(), PublishError> {
    // kind+queued+project 三闸保持原语义（未知/已答/跨项目卡都报 UnknownQuestion）
    crate::cards::get_queued(db, qid, crate::cards::CardKind::Publish)
        .ok()
        .filter(|c| c.project_id == project_id)
        .ok_or_else(|| PublishError::UnknownQuestion(qid.into()))?;
    crate::cards::answer(db, qid, "owner")?;
    db.append_event(
        project_id,
        EventKind::PublishRejected,
        json!({"question_id": qid}),
        None,
        None,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;
    use crate::tools::Registry;

    fn setup() -> (Db, tempfile::TempDir, tempfile::TempDir) {
        let work = tempfile::tempdir().unwrap();
        crate::git::init(work.path(), "main").unwrap();
        std::fs::write(work.path().join("f.txt"), "x").unwrap();
        crate::git::commit_all(work.path(), "init work").unwrap();
        // 裸仓当远端
        let remote = tempfile::tempdir().unwrap();
        crate::git::run(remote.path(), &["init", "--bare", "-b", "main"]).unwrap();
        crate::git::run(
            work.path(),
            &["remote", "add", "origin", &remote.path().to_string_lossy()],
        )
        .unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p',?1,'x','pack')",
                [work.path().to_string_lossy().to_string()],
            )
            .unwrap();
        (db, work, remote)
    }

    #[test]
    fn publish_not_in_tool_registry() {
        // Agent 面没有发布工具：任何角色、任何自治档都无法触发
        let reg = Registry::builtin();
        assert!(!reg.defs().iter().any(|d| d.name == "remote_publish"));
    }

    #[test]
    fn request_queues_card_and_confirm_pushes() {
        let (db, _w, r) = setup();
        let store = MemoryStore::default();
        store.set("publish/origin", "token-x").unwrap();
        let qid = request(&db, "p", "origin").unwrap();
        let v = serde_json::to_value(confirm(&db, "p", &qid, &store).unwrap()).unwrap();
        assert_eq!(v["remote"], "origin");
        // 远端确实收到了 main
        let remote_head = crate::git::run(r.path(), &["rev-parse", "main"]).unwrap();
        let local_head = crate::git::run(
            std::path::Path::new(
                &db.conn()
                    .query_row("SELECT dir FROM projects WHERE id='p'", [], |r| {
                        r.get::<_, String>(0)
                    })
                    .unwrap(),
            ),
            &["rev-parse", "main"],
        )
        .unwrap();
        assert_eq!(remote_head, local_head);
        // 轨迹：request + confirmed
        let kinds: Vec<String> = db
            .conn()
            .prepare("SELECT kind FROM events WHERE project_id='p' ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(kinds.contains(&"publish_requested".to_string()));
        assert!(kinds.contains(&"publish_confirmed".to_string()));
        // 凭据明文不在轨迹里
        assert!(crate::credentials::leak_scan(&db, "p", "token-x")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn missing_credential_fails_closed() {
        let (db, _w, _r) = setup();
        let store = MemoryStore::default(); // 没设凭据
        let qid = request(&db, "p", "origin").unwrap();
        let err = confirm(&db, "p", &qid, &store).unwrap_err();
        assert!(err.to_string().contains("publish/origin"));
        // 失败落轨迹
        let kinds: Vec<String> = db
            .conn()
            .prepare("SELECT kind FROM events WHERE project_id='p'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(kinds.contains(&"publish_failed".to_string()));
    }

    #[test]
    fn reject_marks_answered_no_push() {
        let (db, _w, r) = setup();
        let qid = request(&db, "p", "origin").unwrap();
        reject(&db, "p", &qid).unwrap();
        // 远端无 main 分支——什么都没发出去
        assert!(crate::git::run(r.path(), &["rev-parse", "main"]).is_err());
    }
}
