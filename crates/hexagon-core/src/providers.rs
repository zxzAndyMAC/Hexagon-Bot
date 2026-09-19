//! 供应商配置存储 + 工厂：供应商 → 槽位 → 真实 HTTP 供应商的两级接线。
//!
//! 模型（对齐 Cherry Studio 的供应商中心视图）：
//! - **供应商** `ProviderDef`：市面常见供应商或自定义实例——kind/base_url/模型目录/
//!   启用开关；API key 按供应商存 keychain（`provider/<id>`，一键喂全部槽位）。
//! - **槽位绑定** `slots`：模型槽 → {provider_id, model}；回合内核按槽找供应商。
//!   `default` 槽兜底所有未绑定槽（同 `providers.get(slot).or(get("default"))`）。
//!
//! 文件 `~/.config/hexagon/providers.json`（`HEXAGON_PROVIDERS_PATH` 可覆盖）只放
//! 非密字段；key 永不落盘、永不回传 UI。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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
    #[error("未知供应商：{0}")]
    UnknownProvider(String),
    #[error("http: {0}")]
    Http(String),
}

/// 模型目录条目：拉取/手添的模型 + 能力标记。
/// caps 词表：web（联网）vision（视觉）reasoning（推理）tools（工具调用）free（免费）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEntry {
    pub id: String,
    /// 展示名（空 = 显示 id）。
    #[serde(default)]
    pub name: Option<String>,
    /// 分组名（Cherry 按 id 前缀分组；空 = 按 id 前缀自动归）。
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub caps: Vec<String>,
}

/// 一个供应商实例。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderDef {
    /// 稳定 id（slug）：key 名 `provider/<id>`、槽位绑定都按它引用。
    pub id: String,
    /// 展示名（OpenRouter / 深度求索 / 自建网关…）。
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<ModelEntry>,
    /// ON 开关：关掉的供应商绑定不解析（等效未配置，fail-closed）。
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 槽位 → 供应商+模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SlotBinding {
    pub provider_id: String,
    pub model: String,
}

/// providers.json 文档。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderDoc {
    #[serde(default)]
    pub providers: Vec<ProviderDef>,
    #[serde(default)]
    pub slots: HashMap<String, SlotBinding>,
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

/// 读文档；文件不存在 = 空文档（首次启动常见路径，不算错）。
pub fn load() -> Result<ProviderDoc, ProvidersError> {
    let path = providers_path();
    if !path.exists() {
        return Ok(ProviderDoc::default());
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn write_doc(doc: &ProviderDoc) -> Result<(), ProvidersError> {
    let path = providers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(doc)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// upsert 供应商（id 为主键）。空字段拒绝。
pub fn save_provider(def: &ProviderDef) -> Result<(), ProvidersError> {
    if def.id.trim().is_empty() {
        return Err(ProvidersError::EmptyField("id"));
    }
    if def.name.trim().is_empty() {
        return Err(ProvidersError::EmptyField("name"));
    }
    if def.base_url.trim().is_empty() {
        return Err(ProvidersError::EmptyField("base_url"));
    }
    let mut doc = load()?;
    doc.providers.retain(|p| p.id != def.id);
    doc.providers.push(def.clone());
    write_doc(&doc)
}

/// 删供应商 + 级联解绑指向它的槽位（悬空绑定只会让槽位不就绪，不如清掉）。
/// keychain 不动（key 可能还被同名别处用——本来也只按 id 存一份）。
pub fn delete_provider(id: &str) -> Result<(), ProvidersError> {
    let mut doc = load()?;
    doc.providers.retain(|p| p.id != id);
    doc.slots.retain(|_, b| b.provider_id != id);
    write_doc(&doc)
}

/// 槽位绑定/改绑。供应商必须存在。
pub fn set_binding(slot: &str, provider_id: &str, model: &str) -> Result<(), ProvidersError> {
    let mut doc = load()?;
    if !doc.providers.iter().any(|p| p.id == provider_id) {
        return Err(ProvidersError::UnknownProvider(provider_id.into()));
    }
    doc.slots.insert(
        slot.to_string(),
        SlotBinding {
            provider_id: provider_id.into(),
            model: model.into(),
        },
    );
    write_doc(&doc)
}

/// 解绑槽位。
pub fn remove_binding(slot: &str) -> Result<(), ProvidersError> {
    let mut doc = load()?;
    doc.slots.remove(slot);
    write_doc(&doc)
}

/// 槽位就绪判定（给向导/创建闸）：有绑定 + 供应商存在且启用 + key 已存。
/// `default` 槽的绑定兜底任何槽。
pub fn slot_ready(doc: &ProviderDoc, store: &dyn CredentialStore, slot: &str) -> bool {
    let bound = doc.slots.get(slot).or_else(|| doc.slots.get("default"));
    let Some(b) = bound else { return false };
    let Some(p) = doc.providers.iter().find(|p| p.id == b.provider_id) else {
        return false;
    };
    p.enabled
        && store
            .get(&crate::credentials::provider_key_name(&p.id))
            .ok()
            .flatten()
            .is_some()
}

/// 从配置造一个真实供应商；key 在调用时才从 creds 取（轮换不用重启）。
pub fn make_provider(
    def: &ProviderDef,
    model: &str,
    creds: Arc<dyn CredentialStore>,
) -> Arc<dyn ModelProvider> {
    Arc::new(HttpProvider::new(
        def.kind.clone(),
        def.base_url.clone(),
        model.to_string(),
        crate::credentials::provider_key_name(&def.id),
        creds,
    ))
}

/// 按槽位绑定注册进 Workbench（壳层 open 后调用）：
/// 每个绑定槽 → 其供应商的 HttpProvider；未绑定/供应商缺失/停用 = 不注册（fail-closed）。
pub fn register_all(wb: &mut crate::api::Workbench, creds: Arc<dyn CredentialStore>) {
    let Ok(doc) = load() else { return };
    for (slot, b) in &doc.slots {
        if let Some(p) = doc.providers.iter().find(|p| p.id == b.provider_id) {
            if p.enabled {
                wb.register_provider(slot, make_provider(p, &b.model, creds.clone()));
            }
        }
    }
}

/// 拉取模型目录：GET 端点的 /models 面（OpenAI {base}/models；Anthropic {base}/v1/models）。
/// 返回 ModelEntry（id + 分组 + 推断能力）；4xx/网络错 → Http 错（透传给 UI）。
pub fn fetch_models(def: &ProviderDef, key: &str) -> Result<Vec<ModelEntry>, ProvidersError> {
    let base = def.base_url.trim_end_matches('/');
    let url = match def.kind {
        ProviderKind::Anthropic => format!("{base}/v1/models"),
        ProviderKind::OpenAi => format!("{base}/models"),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let req = agent.get(&url);
    let req = match def.kind {
        ProviderKind::Anthropic => req
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        ProviderKind::OpenAi => req.header("Authorization", &format!("Bearer {key}")),
    };
    let mut resp = req
        .call()
        .map_err(|e| ProvidersError::Http(e.to_string()))?;
    let v: Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| ProvidersError::Http(e.to_string()))?;
    let mut ids: Vec<String> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    Ok(ids
        .into_iter()
        .map(|id| ModelEntry {
            group: Some(group_of(&id)),
            caps: infer_caps(&id),
            name: None,
            id,
        })
        .collect())
}

/// 从模型 id 推断能力标记（拉取时的自动标注，用户可再编辑）。
pub fn infer_caps(id: &str) -> Vec<String> {
    let l = id.to_lowercase();
    let mut caps = Vec::new();
    if l.ends_with(":free") || l.contains("free") {
        caps.push("free".into());
    }
    if [
        "o1",
        "o3",
        "o4",
        "deepseek-r1",
        "qwq",
        "reasoning",
        "thinking",
        "-r1",
    ]
    .iter()
    .any(|k| l.contains(k))
    {
        caps.push("reasoning".into());
    }
    if [
        "vision",
        "gpt-4o",
        "gemini",
        "claude-3",
        "claude-sonnet-4",
        "claude-opus-4",
        "vl",
        "pixtral",
        "image",
    ]
    .iter()
    .any(|k| l.contains(k))
    {
        caps.push("vision".into());
    }
    // 工具调用：现代对话模型默认带，只排除明确的非对话模型
    if !["embed", "whisper", "tts", "dall", "moderation", "audio"]
        .iter()
        .any(|k| l.contains(k))
    {
        caps.push("tools".into());
    }
    caps
}

/// 分组名：id 含 '/' 取前缀，否则按供应商名兜底（UI 侧再美化）。
pub fn group_of(id: &str) -> String {
    id.split('/').next().unwrap_or(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;

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

    fn def(id: &str) -> ProviderDef {
        ProviderDef {
            id: id.into(),
            name: id.into(),
            kind: ProviderKind::OpenAi,
            base_url: "https://api.example.com/v1".into(),
            models: vec![],
            enabled: true,
        }
    }

    /// 供应商 CRUD + 槽位绑定生命周期 + 就绪判定（绑定/启用/key 三要件）。
    #[test]
    fn provider_def_and_binding_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let _g = PathGuard::set(dir.path());
        let store = MemoryStore::default();
        assert!(load().unwrap().providers.is_empty());

        // upsert：id 主键去重
        save_provider(&def("openrouter")).unwrap();
        let mut v2 = def("openrouter");
        v2.base_url = "https://other.example.com".into();
        save_provider(&v2).unwrap();
        let doc = load().unwrap();
        assert_eq!(doc.providers.len(), 1);
        assert_eq!(doc.providers[0].base_url, "https://other.example.com");

        // 绑定 + 就绪判定
        set_binding("chat", "openrouter", "deepseek/chat").unwrap();
        let doc = load().unwrap();
        assert!(!slot_ready(&doc, &store, "chat")); // 无 key 不就绪
        store
            .set(&crate::credentials::provider_key_name("openrouter"), "k")
            .unwrap();
        assert!(slot_ready(&doc, &store, "chat"));
        // default 兜底：绑了 default 的槽对任何槽位都算就绪
        set_binding("default", "openrouter", "m").unwrap();
        let doc = load().unwrap();
        assert!(slot_ready(&doc, &store, "vision"));

        // 停用 → 不就绪（fail-closed）
        let mut off = def("openrouter");
        off.enabled = false;
        save_provider(&off).unwrap();
        let doc = load().unwrap();
        assert!(!slot_ready(&doc, &store, "chat"));
        assert!(!slot_ready(&doc, &store, "vision"));

        // 删供应商 → 级联清绑定
        save_provider(&def("openrouter")).unwrap();
        delete_provider("openrouter").unwrap();
        let doc = load().unwrap();
        assert!(doc.providers.is_empty());
        assert!(doc.slots.is_empty());

        // 幽灵供应商绑定拒
        assert!(matches!(
            set_binding("chat", "ghost", "m"),
            Err(ProvidersError::UnknownProvider(_))
        ));
        // 空字段拒
        let mut bad = def(" ");
        bad.base_url = "ok".into();
        assert!(matches!(
            save_provider(&bad),
            Err(ProvidersError::EmptyField("id"))
        ));
    }

    /// 能力推断：免费/推理/视觉/工具各归位，非对话模型不给 tools。
    #[test]
    fn infer_caps_marks_expected() {
        assert_eq!(
            infer_caps("deepseek/deepseek-r1:free"),
            ["free", "reasoning", "tools"]
        );
        assert!(infer_caps("google/gemini-2.5-flash").contains(&"vision".to_string()));
        assert!(infer_caps("openai/gpt-4o").contains(&"vision".to_string()));
        let emb = infer_caps("openai/text-embedding-3-large");
        assert!(!emb.contains(&"tools".to_string()));
        assert!(infer_caps("meta-llama/llama-3.1-8b").contains(&"tools".to_string()));
        assert_eq!(group_of("google/gemini-2.5-flash"), "google");
    }
}
