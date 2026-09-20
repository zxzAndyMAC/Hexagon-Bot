//! 决策卡队列：`pending_questions` 表的唯一属主（ADR 0053 / arch-review 票 04）。
//!
//! 之前全仓 14 个文件各写各的 SQL——kind 词表、状态机、幂等键、裁决人
//! 四件事散在工具管线/编排/提案/发布/安装五处（诊断卡 D04/D09/D14）。
//! 本模块收口：enqueue/answer/annotate/queued 四个动词 + 读模型；
//! `rg "pending_questions"` 在本模块外只允许命中 schema 迁移与注释。
//!
//! 词表对齐 schema CHECK（migrations 0001/0006/0007）：
//! kind ∈ {permission, stamp, escalation, publish, recovery, install}
//! state ∈ {queued, answered, expired}
//!
//! 注意 kind='stamp' 是过载的：阶段盖章卡（payload={stage,run_id}）与
//! 提案确认卡（payload={proposal_id,...}）共用一个 kind——票 04 只收口
//! 不拆分，拆分是后续 ADR 的事（payload 形状已由调用方各自解读）。

use crate::db::Db;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum CardsError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("question not found: {0}")]
    NotFound(String),
    #[error("question {qid} not queued (state={state})")]
    NotQueued { qid: String, state: String },
}

/// 卡种（schema CHECK 词表）。提案确认卡搭 Stamp——见模块文档过载注记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Permission,
    Stamp,
    Escalation,
    Recovery,
    Install,
    Publish,
}

impl CardKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Stamp => "stamp",
            Self::Escalation => "escalation",
            Self::Recovery => "recovery",
            Self::Install => "install",
            Self::Publish => "publish",
        }
    }
}

/// 卡状态机。合法迁移只有 queued→answered / queued→expired；
/// 本模块不提供 answered→queued 或 *_→expired 之外的出口（无此 API 即证明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardState {
    Queued,
    Answered,
    Expired,
}

impl CardState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Answered => "answered",
            Self::Expired => "expired",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "answered" => Self::Answered,
            "expired" => Self::Expired,
            _ => Self::Queued,
        }
    }
}

/// 一行卡的读取视图。
#[derive(Debug, Clone)]
pub struct Card {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub agent_id: Option<String>,
    pub payload: Value,
    pub state: CardState,
    pub idem_key: Option<String>,
    pub answered_by: Option<String>,
}

// ---------- 四个动词 ----------

/// 入队一张卡：qid 由 id_counters 发号。返回 qid。
/// `idem_key` 只有必问卡用（幂等索引 idx_pending_idem 只管 queued）。
/// 事件不入本函数——各卡种的 asked 事件 kind/payload 不同，归调用方。
pub fn enqueue(
    db: &Db,
    project_id: &str,
    agent_id: Option<&str>,
    kind: CardKind,
    payload: Value,
    idem_key: Option<&str>,
) -> Result<String, CardsError> {
    let qid = format!("q{}", db.next_id("q")?);
    db.conn().execute(
        "INSERT INTO pending_questions (id, project_id, agent_id, kind, payload, idem_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            qid,
            project_id,
            agent_id,
            kind.as_str(),
            payload.to_string(),
            idem_key,
        ],
    )?;
    Ok(qid)
}

/// 销卡：queued→answered（写 answered_at + answered_by 一次性补齐——
/// 历史上有几处只写 answered_by 漏了 answered_at，收口后统一补全）。
/// 不挡重答：调用方先做 get_queued/get 判定；本函数只执行迁移。
pub fn answer(db: &Db, qid: &str, by: &str) -> Result<(), CardsError> {
    db.conn().execute(
        "UPDATE pending_questions SET state='answered', answered_at=datetime('now'),
         answered_by=?2 WHERE id=?1",
        rusqlite::params![qid, by],
    )?;
    Ok(())
}

/// 载荷补丁：把 fields 的键值合进卡 payload（读-改-写，等价于
/// 历史上 reviewer 的整列覆写与 judge 的 json_set 原地位补丁）。
pub fn annotate(db: &Db, qid: &str, fields: &[(&str, Value)]) -> Result<(), CardsError> {
    let mut card = get(db, qid)?;
    for (k, v) in fields {
        card.payload[*k] = v.clone();
    }
    db.conn().execute(
        "UPDATE pending_questions SET payload=?1 WHERE id=?2",
        rusqlite::params![card.payload.to_string(), qid],
    )?;
    Ok(())
}

/// 条件批量补丁（judge 扫尾用）：scope 内所有 queued 卡同打 fields。
/// 单语句 json_set 保持原子的原地更新语义。
pub fn annotate_queued_where(
    db: &Db,
    project_id: &str,
    kind: CardKind,
    payload_key: &str,
    payload_val: &str,
    fields: &[(&str, Value)],
) -> Result<usize, CardsError> {
    let mut sets = String::from("payload = json_set(payload");
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    for (i, (k, v)) in fields.iter().enumerate() {
        sets.push_str(&format!(", '$.{k}', json(?{})", i + 4));
        params.push(serde_json::to_string(v)?.into());
    }
    sets.push(')');
    let sql = format!(
        "UPDATE pending_questions SET {sets}
         WHERE project_id=?1 AND kind=?2 AND state='queued'
           AND json_extract(payload,'$.{payload_key}')=?3"
    );
    let mut all: Vec<rusqlite::types::Value> = vec![
        project_id.to_string().into(),
        kind.as_str().to_string().into(),
        payload_val.to_string().into(),
    ];
    all.extend(params);
    let n = db
        .conn()
        .execute(&sql, rusqlite::params_from_iter(all.iter()))?;
    Ok(n)
}

/// 读模型：项目全部 queued 卡（壳层 pending_questions 命令的数据源）。
pub fn queued(db: &Db, project_id: &str) -> Result<Vec<Value>, CardsError> {
    let mut st = db.conn().prepare(
        "SELECT id, kind, payload, state FROM pending_questions
         WHERE project_id=?1 AND state='queued' ORDER BY created_at",
    )?;
    let rows = st
        .query_map([project_id], |r| {
            Ok(
                serde_json::json!({"id": r.get::<_,String>(0)?, "kind": r.get::<_,String>(1)?,
                      "payload": r.get::<_,String>(2)?, "state": r.get::<_,String>(3)?}),
            )
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------- 读路径（各调用方的既有查询形状） ----------

/// 按 id 取卡（任意状态）。
pub fn get(db: &Db, qid: &str) -> Result<Card, CardsError> {
    db.conn()
        .query_row(
            "SELECT id, project_id, kind, agent_id, payload, state, idem_key, answered_by
             FROM pending_questions WHERE id=?1",
            [qid],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .map_err(|_| CardsError::NotFound(qid.into()))
        .map(
            |(id, project_id, kind, agent_id, payload, state, idem_key, answered_by)| Card {
                id,
                project_id,
                kind,
                agent_id,
                payload: serde_json::from_str(&payload).unwrap_or_default(),
                state: CardState::from_str(&state),
                idem_key,
                answered_by,
            },
        )
}

/// 按 id + kind + queued 态取卡（裁决入口的标准前置闸）。
pub fn get_queued(db: &Db, qid: &str, kind: CardKind) -> Result<Card, CardsError> {
    let card = get(db, qid)?;
    if card.kind != kind.as_str() {
        return Err(CardsError::NotFound(qid.into()));
    }
    if card.state != CardState::Queued {
        return Err(CardsError::NotQueued {
            qid: qid.into(),
            state: card.state.as_str().into(),
        });
    }
    Ok(card)
}

/// 队列里某 kind 的第一张待决卡（按 created_at）。
pub fn first_queued(
    db: &Db,
    project_id: &str,
    kind: CardKind,
) -> Result<Option<String>, CardsError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT id FROM pending_questions
             WHERE project_id=?1 AND kind=?2 AND state='queued' ORDER BY created_at LIMIT 1",
            rusqlite::params![project_id, kind.as_str()],
            |r| r.get(0),
        )
        .ok())
}

/// 某 kind 全部 queued 卡 id。
pub fn queued_ids(db: &Db, project_id: &str, kind: CardKind) -> Result<Vec<String>, CardsError> {
    let mut st = db.conn().prepare(
        "SELECT id FROM pending_questions WHERE project_id=?1 AND kind=?2 AND state='queued'",
    )?;
    let rows = st
        .query_map(rusqlite::params![project_id, kind.as_str()], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 某 kind 的 queued 计数（kind=None → 全部 queued）。
pub fn count_queued(db: &Db, project_id: &str, kind: Option<CardKind>) -> Result<i64, CardsError> {
    let n = db.conn().query_row(
        "SELECT COUNT(*) FROM pending_questions
         WHERE project_id=?1 AND state='queued' AND (?2 IS NULL OR kind=?2)",
        rusqlite::params![project_id, kind.map(|k| k.as_str())],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// queued 卡按 kind 分组计数（autonomy 待办读模型）。
pub fn queued_kind_counts(db: &Db, project_id: &str) -> Result<Vec<(String, i64)>, CardsError> {
    let mut st = db.conn().prepare(
        "SELECT kind, COUNT(*) FROM pending_questions
         WHERE project_id=?1 AND state='queued' GROUP BY kind",
    )?;
    let rows = st
        .query_map([project_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 幂等键查重（票 11）：同 agent+idem_key 的最新一张 permission 卡。
/// 命中语义（queued 复用/answered 沿用裁决/expired 不算）由调用方解释。
pub fn find_by_idem(db: &Db, agent_id: &str, idem_key: &str) -> Result<Option<Card>, CardsError> {
    let row = db
        .conn()
        .query_row(
            "SELECT id, project_id, kind, agent_id, payload, state, idem_key, answered_by
             FROM pending_questions
             WHERE agent_id=?1 AND idem_key=?2 AND kind='permission'
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![agent_id, idem_key],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .ok();
    let Some((id, project_id, kind, agent_id, payload, state, idem, answered_by)) = row else {
        return Ok(None);
    };
    Ok(Some(Card {
        id,
        project_id,
        kind,
        agent_id,
        payload: serde_json::from_str(&payload).unwrap_or_default(),
        state: CardState::from_str(&state),
        idem_key: idem,
        answered_by,
    }))
}

/// scope 销卡（recovery 走这个）：queued 且 payload 某键匹配 → answered。
/// 返回销掉的张数。
pub fn answer_queued_where(
    db: &Db,
    project_id: &str,
    kind: CardKind,
    payload_key: &str,
    payload_val: &str,
    by: &str,
) -> Result<usize, CardsError> {
    let n = db.conn().execute(
        "UPDATE pending_questions SET state='answered', answered_at=datetime('now'), answered_by=?5
         WHERE project_id=?1 AND kind=?2 AND state='queued'
           AND json_extract(payload,?3)=?4",
        rusqlite::params![
            project_id,
            kind.as_str(),
            format!("$.{payload_key}"),
            payload_val,
            by
        ],
    )?;
    Ok(n)
}

/// judge 扫尾的提案×卡联查：awaiting_stamp 且其 queued stamp 卡缺
/// judge_verdict/judge_skip 的提案 id 列表。
pub fn unjudged_stamp_proposals(db: &Db, project_id: &str) -> Result<Vec<String>, CardsError> {
    let mut st = db.conn().prepare(
        "SELECT p.id FROM proposals p
         JOIN pending_questions q ON q.project_id=p.project_id
           AND q.kind='stamp' AND q.state='queued'
           AND json_extract(q.payload,'$.proposal_id')=p.id
         WHERE p.project_id=?1 AND p.status='awaiting_stamp'
           AND json_extract(q.payload,'$.judge_verdict') IS NULL
           AND json_extract(q.payload,'$.judge_skip') IS NULL",
    )?;
    let ids = st
        .query_map([project_id], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(ids)
}

/// 泄漏扫描（credentials::leak_scan 的卡片分片）：payload 含明文的卡 id。
pub fn ids_with_payload_like(
    db: &Db,
    project_id: &str,
    needle: &str,
) -> Result<Vec<String>, CardsError> {
    let mut st = db.conn().prepare(
        "SELECT CAST(id AS TEXT) FROM pending_questions
         WHERE project_id=?1 AND payload LIKE ?2",
    )?;
    let ids = st
        .query_map(rusqlite::params![project_id, format!("%{needle}%")], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role) VALUES ('a1','p1','后端'),('a2','p1','前端')",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn state_machine_no_illegal_paths() {
        let db = db();
        let qid = enqueue(
            &db,
            "p1",
            Some("a1"),
            CardKind::Permission,
            serde_json::json!({"tool":"bash"}),
            None,
        )
        .unwrap();
        // queued → answered 合法
        answer(&db, &qid, "owner").unwrap();
        let card = get(&db, &qid).unwrap();
        assert_eq!(card.state, CardState::Answered);
        // answered 卡不再被 get_queued 认——没有回到 queued 的 API 路径
        assert!(matches!(
            get_queued(&db, &qid, CardKind::Permission),
            Err(CardsError::NotQueued { .. })
        ));
        // 错误 kind 也拿不到（kind 闸）
        assert!(matches!(
            get_queued(&db, &qid, CardKind::Stamp),
            Err(CardsError::NotFound(_))
        ));
    }

    #[test]
    fn annotate_merges_fields_and_preserves_rest() {
        let db = db();
        let qid = enqueue(
            &db,
            "p1",
            None,
            CardKind::Stamp,
            serde_json::json!({"proposal_id":"pr1","surface":"pack"}),
            None,
        )
        .unwrap();
        annotate(
            &db,
            &qid,
            &[
                ("judge_verdict", Value::from("reject")),
                ("judge_advice", Value::from("缺证据")),
            ],
        )
        .unwrap();
        let card = get(&db, &qid).unwrap();
        assert_eq!(card.payload["judge_verdict"], "reject");
        assert_eq!(card.payload["proposal_id"], "pr1"); // 其余字段不动
    }

    #[test]
    fn annotate_queued_where_only_touches_scope() {
        let db = db();
        let hit = enqueue(
            &db,
            "p1",
            None,
            CardKind::Stamp,
            serde_json::json!({"proposal_id":"pr1"}),
            None,
        )
        .unwrap();
        let miss = enqueue(
            &db,
            "p1",
            None,
            CardKind::Stamp,
            serde_json::json!({"proposal_id":"pr2"}),
            None,
        )
        .unwrap();
        let n = annotate_queued_where(
            &db,
            "p1",
            CardKind::Stamp,
            "proposal_id",
            "pr1",
            &[("judge_skip", Value::from("no-evidence"))],
        )
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(get(&db, &hit).unwrap().payload["judge_skip"], "no-evidence");
        assert!(get(&db, &miss).unwrap().payload["judge_skip"].is_null());
        // 已答卡不在 scope 内
        answer(&db, &hit, "owner").unwrap();
        let n = annotate_queued_where(
            &db,
            "p1",
            CardKind::Stamp,
            "proposal_id",
            "pr1",
            &[("judge_skip", Value::from("again"))],
        )
        .unwrap();
        assert_eq!(n, 0);
        assert_eq!(get(&db, &hit).unwrap().payload["judge_skip"], "no-evidence");
    }

    #[test]
    fn idem_lookup_finds_latest_permission_card() {
        let db = db();
        let q1 = enqueue(
            &db,
            "p1",
            Some("a1"),
            CardKind::Permission,
            serde_json::json!({"tool":"bash"}),
            Some("sr1:0:bash:abc"),
        )
        .unwrap();
        assert_eq!(
            find_by_idem(&db, "a1", "sr1:0:bash:abc")
                .unwrap()
                .unwrap()
                .id,
            q1
        );
        assert!(find_by_idem(&db, "a1", "other").unwrap().is_none());
        // 注意：idx_pending_idem 是 (agent, idem_key) 全局唯一——不区分 kind。
        // 同 agent 同 key 的第二张卡（哪怕别的 kind）会撞 UNIQUE；换 agent
        // 才是独立幂等域，此时 kind='permission' 过滤排除它的 stamp 卡。
        enqueue(
            &db,
            "p1",
            Some("a2"),
            CardKind::Stamp,
            serde_json::json!({}),
            Some("sr1:0:bash:abc"),
        )
        .unwrap();
        assert!(find_by_idem(&db, "a2", "sr1:0:bash:abc").unwrap().is_none());
        assert_eq!(
            find_by_idem(&db, "a1", "sr1:0:bash:abc")
                .unwrap()
                .unwrap()
                .id,
            q1
        );
    }
}
