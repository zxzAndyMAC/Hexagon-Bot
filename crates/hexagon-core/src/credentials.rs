//! 凭据钥匙串（票 15）：系统 keychain 存取，核内一律按名字引用。
//!
//! - 命名约定：`model/<slot>` 模型 API key、`publish/<target>` 发布凭据；
//! - 明文只在调用瞬间内存中存在——工具层已拦凭据文件读写（is_credential_path），
//!   scrub_input 保证事件/日志不落敏感值；
//! - 钥匙串不可用 = 明确错误（不降级写配置文件明文——那是把机密换地方丢）；
//! - `MemoryStore` 供测试；`OsKeychain` 走 keyring crate（macOS Keychain /
//!   Windows Credential Manager / Linux Secret Service）。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::db::Db;

#[derive(Debug, thiserror::Error)]
pub enum CredError {
    /// 缺密钥：向导/设置补齐或拿掉该角色才能开跑
    #[error("missing credential: {0}")]
    Missing(String),
    #[error("keychain unavailable: {0}")]
    Store(String),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
}

pub trait CredentialStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, CredError>;
    fn set(&self, name: &str, secret: &str) -> Result<(), CredError>;
    fn delete(&self, name: &str) -> Result<(), CredError>;
}

/// 模型槽 → 凭据名。名字进配置/授权表，值永不。
pub fn model_key_name(slot: &str) -> String {
    format!("model/{slot}")
}

/// 供应商 → 凭据名：key 按供应商存（一键喂其名下全部模型/槽位）。
pub fn provider_key_name(provider_id: &str) -> String {
    format!("provider/{provider_id}")
}

/// OS 钥匙串实现（service = "dev.hexagon.bot"）。
pub struct OsKeychain;

impl CredentialStore for OsKeychain {
    fn get(&self, name: &str) -> Result<Option<String>, CredError> {
        match keyring::Entry::new("dev.hexagon.bot", name) {
            Ok(e) => match e.get_password() {
                Ok(s) => Ok(Some(s)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(CredError::Store(e.to_string())),
            },
            Err(e) => Err(CredError::Store(e.to_string())),
        }
    }
    fn set(&self, name: &str, secret: &str) -> Result<(), CredError> {
        keyring::Entry::new("dev.hexagon.bot", name)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| CredError::Store(e.to_string()))
    }
    fn delete(&self, name: &str) -> Result<(), CredError> {
        match keyring::Entry::new("dev.hexagon.bot", name) {
            Ok(e) => match e.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(CredError::Store(e.to_string())),
            },
            Err(e) => Err(CredError::Store(e.to_string())),
        }
    }
}

/// 内存实现：测试/无系统钥匙串环境下的接缝。
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<HashMap<String, String>>,
}

impl CredentialStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<String>, CredError> {
        Ok(self.map.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, secret: &str) -> Result<(), CredError> {
        self.map.lock().unwrap().insert(name.into(), secret.into());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), CredError> {
        self.map.lock().unwrap().remove(name);
        Ok(())
    }
}

/// 取某 Agent 模型槽对应的 key 明文（调用瞬间用）。缺 = Missing(凭据名)。
pub fn require_model_key(
    db: &Db,
    agent_id: &str,
    store: &dyn CredentialStore,
) -> Result<String, CredError> {
    let slot: Option<String> = db.conn().query_row(
        "SELECT model_slot FROM agents WHERE id=?1",
        [agent_id],
        |r| r.get(0),
    )?;
    let slot = slot.ok_or_else(|| CredError::Missing(format!("model_slot for {agent_id}")))?;
    let name = model_key_name(&slot);
    store.get(&name)?.ok_or(CredError::Missing(name))
}

/// 排查断言帮手：扫描项目全部事件/消息/pending_questions payload，
/// 返回包含该明文的位置清单（空 = 干净）。测试与导出前自检用。
pub fn leak_scan(db: &Db, project_id: &str, secret: &str) -> Result<Vec<String>, CredError> {
    if secret.is_empty() {
        return Ok(vec![]);
    }
    let mut hits = Vec::new();
    for (table, col) in [
        ("events", "payload"),
        ("messages", "body"),
        ("artifacts", "path"),
    ] {
        let sql =
            format!("SELECT CAST(id AS TEXT) FROM {table} WHERE project_id=?1 AND {col} LIKE ?2");
        let mut st = db.conn().prepare(&sql)?;
        let ids: Vec<String> = st
            .query_map(rusqlite::params![project_id, format!("%{secret}%")], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<_, _>>()?;
        for id in ids {
            hits.push(format!("{table}:{id}"));
        }
    }
    // 卡片分片走属主 API（票 04）：表内查询细节不外泄
    for id in crate::cards::ids_with_payload_like(db, project_id, secret)? {
        hits.push(format!("pending_questions:{id}"));
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;
    use crate::tools::Registry;
    use crate::tools::ToolContext;
    use crate::turn::{run_turn, text_response, TurnOutcome};

    fn setup() -> (Db, ToolContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status, model_slot)
                 VALUES ('a0','p','后端','active','chat')",
                [],
            )
            .unwrap();
        let ctx = ToolContext {
            project_id: "p".into(),
            agent_id: "a0".into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: Default::default(),
        };
        (db, ctx, dir)
    }

    #[test]
    fn memory_store_roundtrip_and_missing() {
        let s = MemoryStore::default();
        assert!(s.get("model/chat").unwrap().is_none());
        s.set("model/chat", "sk-secret-123").unwrap();
        assert_eq!(
            s.get("model/chat").unwrap().as_deref(),
            Some("sk-secret-123")
        );
        s.delete("model/chat").unwrap();
        assert!(s.get("model/chat").unwrap().is_none());
    }

    #[test]
    fn missing_key_is_named_not_plaintext() {
        let (db, _ctx, _d) = setup();
        let s = MemoryStore::default();
        let err = require_model_key(&db, "a0", &s).unwrap_err();
        // 错误里是凭据名，不是任何值
        assert_eq!(err.to_string(), "missing credential: model/chat");
    }

    #[test]
    fn secret_never_enters_trace() {
        let (db, ctx, dir) = setup();
        let store = MemoryStore::default();
        store.set("model/chat", "sk-live-abcdef").unwrap();
        // 取一次 key（模拟模型调用瞬间），然后跑完整回合
        let _k = require_model_key(&db, "a0", &store).unwrap();
        let reg = Registry::builtin();
        let provider = ScriptedProvider::new(vec![
            crate::turn::tool_response(vec![(
                "t1",
                "fs_write",
                serde_json::json!({"path":"src/x.rs","content":"fn x(){}"}),
            )]),
            text_response("done"),
        ]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "写代码").unwrap();
        assert_eq!(out, TurnOutcome::Finished);
        // 全表扫描：事件、消息、问题、产物元数据都不含明文
        let hits = leak_scan(&db, "p", "sk-live-abcdef").unwrap();
        assert!(hits.is_empty(), "leak at {hits:?}");
        // 产物文件本身也不含
        let written = std::fs::read_to_string(dir.path().join("src/x.rs")).unwrap();
        assert!(!written.contains("sk-live-abcdef"));
        // 供应商收到的请求也不含（key 是传输层的事，不进 prompt）
        let recorded = provider.recorded();
        for req in recorded {
            assert!(!format!("{req:?}").contains("sk-live-abcdef"));
        }
    }

    #[test]
    fn leak_scan_flags_injection() {
        let (db, _ctx, _d) = setup();
        db.append_event(
            "p",
            crate::trace::EventKind::System,
            serde_json::json!({"note": "key=sk-x"}),
            None,
            None,
        )
        .unwrap();
        assert_eq!(leak_scan(&db, "p", "sk-x").unwrap(), vec!["events:1"]);
    }
}
