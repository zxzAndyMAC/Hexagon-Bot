//! 供应商配置存储 + 工厂：槽位 → 真实 HTTP 供应商的接线。
//!
//! 配置是**全局**的（模型 key 本来就存全局 keychain `model/<slot>`，不按项目分）：
//! `~/.config/hexagon/providers.json`（`HEXAGON_PROVIDERS_PATH` 可覆盖，测试用）。
//! 配置文件只放非密字段（slot/kind/base_url/model）；key 永远走 CredentialStore，
//! HttpProvider 调用时才取——轮换 key 不用重启。
//!
//! 槽位解析在回合内核：`providers.get(slot).or(get("default"))`——配一个
//! `default` 槽即可兜底全部角色。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::credentials::CredentialStore;
use crate::provider::{HttpProvider, ModelProvider, ProviderKind};

#[derive(Debug, thiserror::Error)]
pub enum ProvidersError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("字段不能为空：{0}")]
    EmptyField(&'static str),
}

/// 一条供应商配置：模型槽 → 端点。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    /// 模型槽名（角色 model_slot / "default" 兜底槽）。
    pub slot: String,
    /// "anthropic"（/v1/messages）或 "openai"（兼容 /chat/completions 全家桶）。
    pub kind: ProviderKind,
    /// 端点根，如 https://api.anthropic.com / https://api.deepseek.com/v1。
    pub base_url: String,
    /// 模型 id，如 claude-sonnet-4-6 / gpt-4o / deepseek-chat。
    pub model: String,
}

fn providers_path() -> PathBuf {
    if let Ok(p) = std::env::var("HEXAGON_PROVIDERS_PATH") {
        return PathBuf::from(p);
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("hexagon").join("providers.json")
}

/// 读全部配置；文件不存在 = 空表（首次启动常见路径，不算错）。
pub fn list() -> Result<Vec<ProviderConfig>, ProvidersError> {
    let path = providers_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn write_all(cfgs: &[ProviderConfig]) -> Result<(), ProvidersError> {
    let path = providers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(cfgs)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 校验并 upsert（slot 为主键）。空字段拒绝；kind 由 serde 枚举约束。
pub fn save(cfg: &ProviderConfig) -> Result<(), ProvidersError> {
    if cfg.slot.trim().is_empty() {
        return Err(ProvidersError::EmptyField("slot"));
    }
    if cfg.base_url.trim().is_empty() {
        return Err(ProvidersError::EmptyField("base_url"));
    }
    if cfg.model.trim().is_empty() {
        return Err(ProvidersError::EmptyField("model"));
    }
    let mut cfgs = list()?;
    cfgs.retain(|c| c.slot != cfg.slot);
    cfgs.push(cfg.clone());
    write_all(&cfgs)
}

/// 按 slot 删配置；keychain 不动（key 可能还被别处用）。
pub fn delete(slot: &str) -> Result<(), ProvidersError> {
    let mut cfgs = list()?;
    cfgs.retain(|c| c.slot != slot);
    write_all(&cfgs)
}

/// 从配置造一个真实供应商；key 在调用时才从 creds 取。
pub fn make_provider(
    cfg: &ProviderConfig,
    creds: Arc<dyn CredentialStore>,
) -> Arc<dyn ModelProvider> {
    Arc::new(HttpProvider::new(
        cfg.kind.clone(),
        cfg.base_url.clone(),
        cfg.model.clone(),
        crate::credentials::model_key_name(&cfg.slot),
        creds,
    ))
}

/// 加载全部配置并按槽注册进 Workbench（壳层 open 后调用）。
pub fn register_all(wb: &mut crate::api::Workbench, creds: Arc<dyn CredentialStore>) {
    if let Ok(cfgs) = list() {
        for cfg in &cfgs {
            wb.register_provider(&cfg.slot, make_provider(cfg, creds.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PathGuard;
    impl PathGuard {
        fn set(dir: &std::path::Path) -> Self {
            std::env::set_var(
                "HEXAGON_PROVIDERS_PATH",
                dir.join("providers.json").to_str().unwrap(),
            );
            PathGuard
        }
    }
    impl Drop for PathGuard {
        fn drop(&mut self) {
            std::env::remove_var("HEXAGON_PROVIDERS_PATH");
        }
    }

    fn cfg(slot: &str) -> ProviderConfig {
        ProviderConfig {
            slot: slot.into(),
            kind: ProviderKind::OpenAi,
            base_url: "https://api.example.com/v1".into(),
            model: "m1".into(),
        }
    }

    /// 配置 CRUD：缺文件=空表、upsert 主键去重、删除收窄、空字段拒。
    #[test]
    fn provider_config_crud() {
        let dir = tempfile::tempdir().unwrap();
        let _g = PathGuard::set(dir.path());
        assert!(list().unwrap().is_empty()); // 无文件不报错

        save(&cfg("chat")).unwrap();
        save(&cfg("default")).unwrap();
        assert_eq!(list().unwrap().len(), 2);

        let mut updated = cfg("chat");
        updated.model = "m2".into();
        save(&updated).unwrap();
        let cfgs = list().unwrap();
        assert_eq!(cfgs.len(), 2); // upsert 不重复
        assert_eq!(cfgs.iter().find(|c| c.slot == "chat").unwrap().model, "m2");

        delete("chat").unwrap();
        assert_eq!(list().unwrap().len(), 1);
        assert_eq!(list().unwrap()[0].slot, "default");

        // 空字段拒
        let mut bad = cfg("x");
        bad.base_url = " ".into();
        assert!(matches!(
            save(&bad),
            Err(ProvidersError::EmptyField("base_url"))
        ));
        let mut bad2 = cfg(" ");
        bad2.base_url = "ok".into();
        assert!(matches!(
            save(&bad2),
            Err(ProvidersError::EmptyField("slot"))
        ));
    }
}
