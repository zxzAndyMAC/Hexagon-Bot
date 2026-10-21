//! 推送（票 16 起称「远程发布」；ADR 0093 / 2026-10-17 负责人裁决改名推送，
//! 「发布」留给执行已登记发布命令的新功能）：显式指令 + 双闸，无自动路径。
//!
//! - **不进工具注册表**——Agent 拿不到 `remote_publish` 工具，任何自治档都
//!   无法触发；唯一入口是负责人的 API 指令（确认卡）。
//! - 双闸：推送确认卡（kind='push' 必问，写明远端与不可逆提示，永不记忆）
//!   + 钥匙串推送凭据 `push/<remote>`，缺凭据即明确失败。
//! - 执行走 git 管道短命令；确认人、时间、目标、结果全落轨迹。
//! - 改名迁移（ADR 0093）：旧卡种 `publish`、旧事件 `publish_*` 由迁移 0064
//!   映射到新名；旧凭据 `publish/<remote>` 走 [`credential`] 兼容读取。

use serde_json::json;

use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::trace::EventKind;

#[derive(Debug, thiserror::Error)]
pub enum PushError {
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
    #[error("unknown push question: {0}")]
    UnknownQuestion(String),
}

/// 推送凭据名（ADR 0093）。名字进轨迹/错误，值永不。
pub fn credential_name(remote: &str) -> String {
    format!("push/{remote}")
}

/// ADR 0093 改名前的凭据名，只供 [`credential`] 兼容读取。
pub fn legacy_credential_name(remote: &str) -> String {
    format!("publish/{remote}")
}

/// 读推送凭据：新名优先，缺则读旧名 `publish/<remote>`（ADR 0093：升级后旧凭据
/// 继续可用，不要求重录）。
///
/// 选兼容读取、不做首读迁移：迁移得在确认路径上写新名再删旧名，钥匙串写一半
/// 失败会丢掉唯一一份凭据；兼容读取零副作用。启动时批量迁移也被否决——钥匙串
/// 不能按前缀枚举名字。缺凭据时报新名，负责人照新名补录。
pub fn credential(
    store: &dyn CredentialStore,
    remote: &str,
) -> Result<Option<String>, crate::credentials::CredError> {
    match store.get(&credential_name(remote))? {
        Some(secret) => Ok(Some(secret)),
        None => store.get(&legacy_credential_name(remote)),
    }
}

fn project_dir(db: &Db, project_id: &str) -> Result<std::path::PathBuf, PushError> {
    Ok(db
        .conn()
        .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
            r.get::<_, String>(0)
        })?
        .into())
}

/// 发起推送：只入队确认卡，不执行任何东西。
pub fn request(db: &Db, project_id: &str, remote: &str) -> Result<String, PushError> {
    let baseline = crate::git::baseline_branch(std::path::Path::new(&db.conn().query_row(
        "SELECT dir FROM projects WHERE id=?1",
        [project_id],
        |r| r.get::<_, String>(0),
    )?))
    .unwrap_or_else(|_| "main".into());
    // Test3/22: the delivered version at request time, so a delegated
    // push can refuse a commit that changed after the request.
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
        crate::cards::CardKind::Push,
        json!({
            "remote": remote,
            "baseline": baseline,
            "commit": commit,
            "commit_count": commit_count,
            "stage_count": stage_count,
            "warning": "推送不可逆：代码与产物将推送到项目外部",
        }),
        None,
    )?;
    db.append_event(
        project_id,
        EventKind::PushRequested,
        json!({"remote": remote, "baseline": baseline, "question_id": qid}),
        None,
        None,
    )?;
    log::info!("push requested: remote={remote} qid={qid}");
    Ok(qid)
}

/// 功能缺口 A6（2026-10-15，decisions#cards 推送卡（原发布卡）「包含 N 个提交 · N 个阶段的产物」）：
/// - 提交数 = 本地远端跟踪分支 `<remote>/<baseline>` 之后、基线上的提交（`rev-list --count`）。
///   没有跟踪分支（从未推送或未抓取）时无从得知远端已有什么，给 null，UI 不写这一半——不猜。
/// - 阶段数 = 已完成（done）且留有产物的阶段名去重数。
fn included(
    db: &Db,
    project_id: &str,
    dir: &std::path::Path,
    remote: &str,
    baseline: &str,
) -> Result<(Option<u64>, u64), PushError> {
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

/// 推送回执（ADR 0054）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PushOutcome {
    pub remote: String,
    pub baseline: String,
    pub output: String,
}

/// 负责人发起推送的结果（owner-followups-1015 #13）：总有一张推送卡；
/// 本项目已允许同一远程 + 分支时宿主当场确认，`pushed` 是回执。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PushRequest {
    pub question_id: String,
    pub pushed: Option<PushOutcome>,
}

/// ⌘K「推送」的目标候选（#13，ADR 0093：不再写死 origin）。
/// `upstream` = 基线分支的上游远程（`branch.<基线>.remote`，且是已配置的远程）；
/// 没有时由负责人在 `remotes` 里选。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PushTargets {
    pub baseline: String,
    pub upstream: Option<String>,
    pub remotes: Vec<String>,
}

pub fn targets(db: &Db, project_id: &str) -> Result<PushTargets, PushError> {
    let dir = project_dir(db, project_id)?;
    let baseline = crate::git::baseline_branch(&dir).unwrap_or_else(|_| "main".into());
    let remotes: Vec<String> = crate::git::run(&dir, &["remote"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .collect();
    // `branch.<b>.remote` 可以是 "."（本地跟踪）或已删掉的远程名——都不是可推的上游，
    // 当没有上游处理，让负责人选；不猜 origin（ADR 0093 后续裁决）。
    let upstream = crate::git::run(
        &dir,
        &["config", "--get", &format!("branch.{baseline}.remote")],
    )
    .ok()
    .map(|r| r.trim().to_string())
    .filter(|r| remotes.contains(r));
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(project_id),
        None,
        None,
        None,
        "push_target",
        if upstream.is_some() {
            "baseline_upstream"
        } else {
            "owner_chooses"
        },
        std::time::Instant::now(),
    );
    Ok(PushTargets {
        baseline,
        upstream,
        remotes,
    })
}

/// 负责人发起推送（#13）：入队推送卡；本项目已「允许本项目」同一（远程, 基线分支）时
/// 宿主以该规则为裁决人当场确认（凭据闸照旧，失败照旧落 PushFailed、不重发）。
/// Agent 终端推送命中同一规则走权限 L4 记忆层——两边一套记住规则。
pub fn request_or_push(
    db: &Db,
    project_id: &str,
    remote: &str,
    store: &dyn CredentialStore,
) -> Result<PushRequest, PushError> {
    let started = std::time::Instant::now();
    let question_id = request(db, project_id, remote)?;
    let baseline = crate::cards::get(db, &question_id)?.payload["baseline"]
        .as_str()
        .unwrap_or("main")
        .to_string();
    let rule = crate::permissions::push_remembered(db, project_id, remote, &baseline)?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(project_id),
        None,
        None,
        None,
        "push_card",
        if rule.is_some() { "remembered" } else { "ask" },
        started,
    );
    let pushed = match rule {
        Some(id) => Some(confirm_by(
            db,
            project_id,
            &question_id,
            store,
            &format!("project_permission:{id}"),
        )?),
        None => None,
    };
    Ok(PushRequest {
        question_id,
        pushed,
    })
}

/// 确认推送：凭据闸 + git push。确认事实先落轨迹，失败单独落 PushFailed。
pub fn confirm(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
) -> Result<PushOutcome, PushError> {
    confirm_by(db, project_id, qid, store, "owner")
}

/// 负责人在推送卡上选「允许本项目」（#13）：先记（远程, 基线分支）规则，再照常确认推送。
/// 记住的是负责人的同意，不随这次推送成败撤回（缺凭据补录后下次直接推）。
/// 只负责人可记：托管走 `confirm_by`，没有这条路（托管不得新增授权，ADR 0086）。
pub fn confirm_remembering(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
) -> Result<PushOutcome, PushError> {
    let card = crate::cards::get_queued(db, qid, crate::cards::CardKind::Push)
        .ok()
        .filter(|c| c.project_id == project_id)
        .ok_or_else(|| PushError::UnknownQuestion(qid.into()))?;
    let remote = card.payload["remote"].as_str().unwrap_or("origin");
    let baseline = card.payload["baseline"].as_str().unwrap_or("main");
    let rule_id = crate::permissions::remember_push(db, project_id, remote, baseline)?;
    db.append_event(
        project_id,
        EventKind::PermissionShapeRemembered,
        json!({ "tool": "bash", "shape": crate::permissions::push_shape(remote, baseline),
                "scope": "project", "project_shared": true, "rule_id": rule_id, "question_id": qid }),
        None,
        None,
    )?;
    confirm(db, project_id, qid, store)
}

/// Test3/22: the same push by a non-owner decider (project delegation).
/// Same credential gate and push; the card is answered before the push, so a
/// failure is recorded and never silently re-sent.
pub fn confirm_by(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
    by: &str,
) -> Result<PushOutcome, PushError> {
    // 卡表读写归 cards.rs（arch-review 票 04）
    let p = crate::cards::get_queued(db, qid, crate::cards::CardKind::Push)
        .map_err(|_| PushError::UnknownQuestion(qid.into()))?
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
    let result = (|| -> Result<PushOutcome, PushError> {
        // 凭据闸：push/<remote>（或旧名）必须存在于钥匙串（值只验证存在，不进命令行/日志）
        credential(store, &remote)?
            .ok_or_else(|| crate::credentials::CredError::Missing(credential_name(&remote)))?;
        let out = crate::git::run(repo, &["push", "-u", &remote, &baseline])?;
        Ok(PushOutcome {
            remote: remote.clone(),
            baseline: baseline.clone(),
            output: out,
        })
    })();

    match result {
        Ok(v) => {
            db.append_event(
                project_id,
                EventKind::PushConfirmed,
                json!({"remote": remote, "baseline": baseline, "question_id": qid, "by": by}),
                None,
                None,
            )?;
            log::info!("push confirmed: {remote} {baseline}");
            Ok(v)
        }
        Err(e) => {
            db.append_event(
                project_id,
                EventKind::PushFailed,
                json!({"remote": remote, "question_id": qid, "error": e.to_string(), "by": by}),
                None,
                None,
            )?;
            log::warn!("push failed: {e}");
            Err(e)
        }
    }
}

/// 拒绝推送：标记已答 + 轨迹。
pub fn reject(db: &Db, project_id: &str, qid: &str) -> Result<(), PushError> {
    reject_by(db, project_id, qid, "owner")
}

/// Test3/22: a push refusal by a non-owner decider.
pub fn reject_by(db: &Db, project_id: &str, qid: &str, by: &str) -> Result<(), PushError> {
    // kind+queued+project 三闸保持原语义（未知/已答/跨项目卡都报 UnknownQuestion）
    crate::cards::get_queued(db, qid, crate::cards::CardKind::Push)
        .ok()
        .filter(|c| c.project_id == project_id)
        .ok_or_else(|| PushError::UnknownQuestion(qid.into()))?;
    crate::cards::answer(db, qid, by)?;
    db.append_event(
        project_id,
        EventKind::PushRejected,
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
    fn push_not_in_tool_registry() {
        // Agent 面没有推送工具：任何角色、任何自治档都无法触发
        let reg = Registry::builtin();
        assert!(!reg.defs().iter().any(|d| d.name == "remote_publish"));
    }

    #[test]
    fn request_queues_card_and_confirm_pushes() {
        let (db, _w, r) = setup();
        let store = MemoryStore::default();
        store.set("push/origin", "token-x").unwrap();
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
        assert!(kinds.contains(&"push_requested".to_string()));
        assert!(kinds.contains(&"push_confirmed".to_string()));
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
        assert!(err.to_string().contains("push/origin"));
        // 失败落轨迹
        let kinds: Vec<String> = db
            .conn()
            .prepare("SELECT kind FROM events WHERE project_id='p'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(kinds.contains(&"push_failed".to_string()));
    }

    #[test]
    fn legacy_publish_credential_still_pushes() {
        // ADR 0093: a credential stored before the rename keeps working, no re-entry.
        let (db, _w, r) = setup();
        let store = MemoryStore::default();
        store.set("publish/origin", "token-old").unwrap();
        let qid = request(&db, "p", "origin").unwrap();
        confirm(&db, "p", &qid, &store).unwrap();
        assert!(crate::git::run(r.path(), &["rev-parse", "main"]).is_ok());
        // The new name wins when both exist.
        store.set("push/origin", "token-new").unwrap();
        assert_eq!(
            credential(&store, "origin").unwrap().as_deref(),
            Some("token-new")
        );
        assert!(crate::credentials::leak_scan(&db, "p", "token-old")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn push_card_counts_unpushed_commits_and_delivered_stages() {
        let (db, work, _r) = setup();
        let store = MemoryStore::default();
        store.set("push/origin", "token-x").unwrap();
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
