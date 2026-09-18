//! SQLite 访问层：连接 + 有序可重放迁移。
//!
//! 迁移即真相：`migrations/NNNN_name.sql` 按文件名序应用，已应用的版本记进
//! `schema_migrations`；从空库可一路重放到最新。

use rusqlite::Connection;
use std::path::Path;

/// 迁移清单：(版本名, SQL)。追加新迁移 = 往数组尾部加一行，禁止改已发布的项。
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("../migrations/0001_init.sql")),
    (
        "0002_rule_stage",
        include_str!("../migrations/0002_rule_stage.sql"),
    ),
    (
        "0003_artifact_content",
        include_str!("../migrations/0003_artifact_content.sql"),
    ),
    (
        "0004_usage_stage",
        include_str!("../migrations/0004_usage_stage.sql"),
    ),
    (
        "0005_agent_globs",
        include_str!("../migrations/0005_agent_globs.sql"),
    ),
    (
        "0006_interrupted",
        include_str!("../migrations/0006_interrupted.sql"),
    ),
    (
        "0007_install",
        include_str!("../migrations/0007_install.sql"),
    ),
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// 打开（必要时创建）项目数据库并迁移到最新。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        Self::init(Connection::open(path)?)
    }

    /// 内存库，测试用。
    pub fn open_in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, DbError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        // 表重建类迁移（如 0006）要求迁移期外键关闭——PRAGMA 在事务外才生效。
        self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        for (version, sql) in MIGRATIONS {
            let applied: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                [version],
                |r| r.get(0),
            )?;
            if applied {
                continue;
            }
            log::info!("applying migration {version}");
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [version],
            )?;
            tx.commit()?;
        }
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 项目内短 id：`{prefix}{n}`，n 按 prefix 单调递增（存 id_counters）。
    pub fn next_id(&self, prefix: &str) -> Result<i64, DbError> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS id_counters (prefix TEXT PRIMARY KEY, n INTEGER NOT NULL)",
            [],
        )?;
        self.conn.execute(
            "INSERT INTO id_counters (prefix, n) VALUES (?1, 1)
             ON CONFLICT(prefix) DO UPDATE SET n = n + 1",
            [prefix],
        )?;
        let n: i64 = self.conn.query_row(
            "SELECT n FROM id_counters WHERE prefix = ?1",
            [prefix],
            |r| r.get(0),
        )?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_names(db: &Db) -> Vec<String> {
        let mut st = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn migrates_from_empty_and_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        // 重放：再迁移一次不应报错也不重复应用
        db.migrate().unwrap();
        let n: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n as usize, MIGRATIONS.len());
    }

    #[test]
    fn covers_every_spec_entity() {
        let db = Db::open_in_memory().unwrap();
        let tables = table_names(&db);
        // spec「逻辑表」逐实体核对（team 由 agents 集合表达）
        for t in [
            "projects",
            "agents",
            "grants",
            "stage_runs",
            "artifacts",
            "events",
            "messages",
            "permission_rules",
            "usage",
            "proposals",
            "pending_questions",
        ] {
            assert!(tables.contains(&t.to_string()), "missing table {t}");
        }
    }

    #[test]
    fn foreign_keys_enforced() {
        let db = Db::open_in_memory().unwrap();
        let r = db.conn().execute(
            "INSERT INTO agents (id, project_id, role) VALUES ('a1','no-such','后端')",
            [],
        );
        assert!(r.is_err());
    }
}
