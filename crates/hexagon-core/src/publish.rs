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

fn project_dir(db: &Db, project_id: &str) -> Result<std::path::PathBuf, PublishError> {
    Ok(db
        .conn()
        .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
            r.get::<_, String>(0)
        })?
        .into())
}

/// 发起发布：只入队确认卡，不执行任何东西。
pub fn request(db: &Db, project_id: &str, remote: &str) -> Result<String, PublishError> {
    let baseline = crate::git::baseline_branch(std::path::Path::new(&db.conn().query_row(
        "SELECT dir FROM projects WHERE id=?1",
        [project_id],
        |r| r.get::<_, String>(0),
    )?))
    .unwrap_or_else(|_| "main".into());
    // Test3/22: the delivered version at request time, so a delegated
    // publish can refuse a commit that changed after the request.
    // The commit of the branch that is actually pushed, not HEAD (review:
    // checking HEAD while pushing the baseline left a gap).
    let dir = project_dir(db, project_id)?;
    let commit = crate::git::run(&dir, &["rev-parse", &baseline])
        .map(|out| out.trim().to_string())
        .ok();
    let (commit_count, stage_count) = included(db, project_id, &dir, remote, &baseline)?;
    let qid = crate::cards::enqueue(
        db,
        project_id,
        None,
        crate::cards::CardKind::Publish,
        json!({
            "remote": remote,
            "baseline": baseline,
            "commit": commit,
            "commit_count": commit_count,
            "stage_count": stage_count,
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

/// 功能缺口 A6（2026-10-15，decisions#cards 发布卡「包含 N 个提交 · N 个阶段的产物」）：
/// - 提交数 = 本地远端跟踪分支 `<remote>/<baseline>` 之后、基线上的提交（`rev-list --count`）。
///   没有跟踪分支（从未推送或未抓取）时无从得知远端已有什么，给 null，UI 不写这一半——不猜。
/// - 阶段数 = 已完成（done）且留有产物的阶段名去重数。
fn included(
    db: &Db,
    project_id: &str,
    dir: &std::path::Path,
    remote: &str,
    baseline: &str,
) -> Result<(Option<u64>, u64), PublishError> {
    let tracking = format!("refs/remotes/{remote}/{baseline}");
    let commits = crate::git::run(dir, &["rev-parse", "--verify", "--quiet", &tracking])
        .ok()
        .and_then(|_| {
            crate::git::run(
                dir,
                &["rev-list", "--count", &format!("{tracking}..{baseline}")],
            )
            .ok()
        })
        .and_then(|out| out.trim().parse::<u64>().ok());
    let stages: i64 = db.conn().query_row(
        "SELECT COUNT(DISTINCT sr.stage_name) FROM artifacts a JOIN stage_runs sr ON sr.id=a.stage_run_id
         WHERE a.project_id=?1 AND sr.state='done'",
        [project_id],
        |r| r.get(0),
    )?;
    Ok((commits, u64::try_from(stages).unwrap_or(0)))
}

/// 发布回执（ADR 0054）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
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
    confirm_by(db, project_id, qid, store, "owner")
}

/// Test3/22: the same publish by a non-owner decider (project delegation).
/// Same credential gate and push; the card is answered before the push, so a
/// failure is recorded and never silently re-sent.
pub fn confirm_by(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
    by: &str,
) -> Result<PublishOutcome, PublishError> {
    // 卡表读写归 cards.rs（arch-review 票 04）
    let p = crate::cards::get_queued(db, qid, crate::cards::CardKind::Publish)
        .map_err(|_| PublishError::UnknownQuestion(qid.into()))?
        .payload;
    let remote = p["remote"].as_str().unwrap_or("origin").to_string();
    let baseline = p["baseline"].as_str().unwrap_or("main").to_string();

    crate::cards::answer(db, qid, by)?;

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
                json!({"remote": remote, "baseline": baseline, "question_id": qid, "by": by}),
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
                json!({"remote": remote, "question_id": qid, "error": e.to_string(), "by": by}),
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
    reject_by(db, project_id, qid, "owner")
}

/// Test3/22: a publish refusal by a non-owner decider.
pub fn reject_by(db: &Db, project_id: &str, qid: &str, by: &str) -> Result<(), PublishError> {
    // kind+queued+project 三闸保持原语义（未知/已答/跨项目卡都报 UnknownQuestion）
    crate::cards::get_queued(db, qid, crate::cards::CardKind::Publish)
        .ok()
        .filter(|c| c.project_id == project_id)
        .ok_or_else(|| PublishError::UnknownQuestion(qid.into()))?;
    crate::cards::answer(db, qid, by)?;
    db.append_event(
        project_id,
        EventKind::PublishRejected,
        json!({"question_id": qid, "by": by}),
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
    fn publish_card_counts_unpushed_commits_and_delivered_stages() {
        let (db, work, _r) = setup();
        let store = MemoryStore::default();
        store.set("publish/origin", "token-x").unwrap();
        let first = request(&db, "p", "origin").unwrap();
        let payload = crate::cards::get(&db, &first).unwrap().payload;
        // 从未推送：没有跟踪分支，提交数未知（null），不猜。
        assert!(payload["commit_count"].is_null());
        assert_eq!(payload["stage_count"], 0);
        confirm(&db, "p", &first, &store).unwrap();
        std::fs::write(work.path().join("g.txt"), "y").unwrap();
        crate::git::commit_all(work.path(), "second").unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO stage_runs (id,project_id,stage_name,seq,state) VALUES ('s1','p','设计',1,'done'),('s2','p','实现',2,'active');
                 INSERT INTO artifacts (id,project_id,path,kind,tier,stage_run_id,version) VALUES
                   ('a1','p','x.md','规格','freeform','s1',1),('a2','p','y.md','规格','freeform','s2',1);",
            )
            .unwrap();
        let second = request(&db, "p", "origin").unwrap();
        let payload = crate::cards::get(&db, &second).unwrap().payload;
        assert_eq!(payload["commit_count"], 1);
        assert_eq!(
            payload["stage_count"], 1,
            "only finished stages with artifacts count"
        );
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
