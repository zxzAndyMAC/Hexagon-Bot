//! 供应商配置存储 + 工厂：供应商 → 槽位 → 真实 HTTP 供应商的两级接线。
//!
//! 模型（对齐 Cherry Studio 的供应商中心视图）：
//! - **供应商** `ProviderDef`：市面常见供应商或自定义实例——kind/base_url/模型目录/
//!   启用开关；API key 按供应商存 keychain（`provider/<id>`，一键喂全部槽位）。
//! - **槽位绑定** `slots`：模型槽 → {provider_id, model}；回合内核按槽找供应商。
//!   `default` 槽兜底所有未绑定槽（统一经 `resolve_slot`，全仓唯一实现——
//!   回退链曾手写复制四处，arch-review 票 01 / 诊断卡 D13）。
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
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
    /// 上下文窗口 tok（context-window 票 02 / ADR 0068）：拉目录时按
    /// 内置前缀表兜底填，未识别留空 → 撞限闸回落 120k 全局上限，
    /// 设置页提示手填。大窗口不放开吃满——120k 仍是刻意纪律上限。
    /// `number | null`：u64 默认被 ts-rs 导成 bigint，与 JSON number 不符。
    #[serde(default)]
    #[ts(type = "number | null")]
    pub context_window: Option<u64>,
    /// 单次响应输出上限 tok：两种请求形状都显式传给端点。
    /// None → 供应商旧默认（Anthropic 8192；OpenAI 形状不传，吃端点默认）。
    #[serde(default)]
    #[ts(type = "number | null")]
    pub max_output: Option<u64>,
}

/// 一个供应商实例。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SlotBinding {
    pub provider_id: String,
    pub model: String,
}

/// providers.json 文档。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
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

/// 槽位回退链（全仓唯一实现，arch-review 票 01 / 诊断卡 D13）：
/// 绑定槽缺失时回退 `default` 槽。调用方不得再手写 `.or_else(get("default"))`。
pub fn resolve_slot<'a, V>(map: &'a HashMap<String, V>, slot: &str) -> Option<&'a V> {
    map.get(slot).or_else(|| map.get("default"))
}

/// 槽位能力集（agent-senses 票 02）：绑定槽 → 模型条目 caps；
/// 模型不在目录 → infer_caps 兜底推断；链断任一环 → 空集。
pub fn caps_for_slot(slot: &str) -> std::collections::HashSet<String> {
    let Ok(doc) = load() else {
        return Default::default();
    };
    let Some(b) = resolve_slot(&doc.slots, slot) else {
        return Default::default();
    };
    let Some(p) = doc.providers.iter().find(|p| p.id == b.provider_id) else {
        return Default::default();
    };
    if let Some(m) = p.models.iter().find(|m| m.id == b.model) {
        return m.caps.iter().cloned().collect();
    }
    infer_caps(&b.model).into_iter().collect()
}

/// 槽位就绪判定（给向导/创建闸）：有绑定 + 供应商存在且启用 + key 已存。
/// `default` 槽的绑定兜底任何槽。
pub fn slot_ready(doc: &ProviderDoc, store: &dyn CredentialStore, slot: &str) -> bool {
    let bound = resolve_slot(&doc.slots, slot);
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
/// 票 02：模型元数据（窗口/输出上限）随实例带上——撞限闸和 max_tokens
/// 都以「实际服务的那个模型」为准，不回去重读配置（槽位回退已由
/// resolve_slot 在挂表时定完，重读会引入配置漂移窗口）。
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
        crate::provider::ModelMeta {
            context_window: window_of(def, model),
            max_output: max_output_of(def, model),
        },
    ))
}

/// 按槽位绑定注册进 Workbench（壳层 open 后调用）：
/// 每个绑定槽 → 其供应商的 HttpProvider；未绑定/供应商缺失/停用 = 不注册（fail-closed）。
/// 按配置把启用的供应商实例化进槽位 map。收 `&mut` 槽位表而非 `&mut Workbench`——
/// 本模块是叶子（配置+工厂），不得摸门面类型（arch-review 票 01 / 诊断卡 D08）。
pub fn register_all(
    providers: &mut HashMap<String, Arc<dyn ModelProvider>>,
    creds: Arc<dyn CredentialStore>,
) {
    let Ok(doc) = load() else { return };
    for (slot, b) in &doc.slots {
        if let Some(p) = doc.providers.iter().find(|p| p.id == b.provider_id) {
            if p.enabled {
                providers.insert(slot.clone(), make_provider(p, &b.model, creds.clone()));
            }
        }
    }
}

/// Jev 没有模型目录接口。用一道最小是非题确认钥匙，目录只放响应里的模型名，
/// 没有则回落 `jev-latest`。
fn fetch_jev_models(def: &ProviderDef, key: &str) -> Result<Vec<ModelEntry>, ProvidersError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let body = serde_json::json!({
        "state": "ping",
        "model": "jev-latest",
        "questions": {
            "ok": { "type": "noul", "instructions": "Is this a connectivity check?" }
        }
    });
    let mut resp = agent
        .post(crate::provider::systemone_url(&def.base_url))
        .header("Authorization", &format!("Bearer {key}"))
        .send_json(&body)
        .map_err(|e| ProvidersError::Http(e.to_string()))?;
    let v: Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| ProvidersError::Http(e.to_string()))?;
    let id = v["model"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("jev-latest")
        .to_string();
    Ok(vec![ModelEntry {
        id,
        name: Some("Jev".into()),
        group: Some("typesafe".into()),
        caps: vec![],
        ..ModelEntry::default()
    }])
}

/// 拉取模型目录：GET 端点的 /models 面（OpenAI {base}/models；Anthropic {base}/v1/models）。
/// 返回 ModelEntry（id + 分组 + 推断能力）；4xx/网络错 → Http 错（透传给 UI）。
pub fn fetch_models(def: &ProviderDef, key: &str) -> Result<Vec<ModelEntry>, ProvidersError> {
    let base = def.base_url.trim_end_matches('/');
    let url = match def.kind {
        ProviderKind::Anthropic => format!("{base}/v1/models"),
        ProviderKind::OpenAi => format!("{base}/models"),
        // Jev 没有 /models。用一道最小选择题确认钥匙，模型名以响应为准。
        ProviderKind::Jev => return fetch_jev_models(def, key),
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
        ProviderKind::Jev => unreachable!("jev returns before the models request"),
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
            // 票 02：前缀表兜底填窗口/输出上限，未识别留空（设置页可手填）。
            context_window: infer_window(&id),
            max_output: infer_max_output(&id),
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
    // 供应商原生搜索（agent-senses 票 04 / ADR 0058-1）：只认 Anthropic
    // 模型族——web_search_20250305 是 Anthropic server tool；OpenAI 形状
    // 缺席属预期不对称（不自建爬虫顶替，能力随供应商切换消失）。
    if l.contains("claude") {
        caps.push("web".into());
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

/// 内置模型元数据表（context-window 票 02 / ADR 0068）：LiteLLM
/// model_prices_and_context_window 登记表的精简 vendor 版——拉目录/解析
/// 槽位时按 id 子串兜底填默认，用户可在设置页改。
/// 行 = (id 子串, 窗口 tok, 输出上限 tok)。顺序敏感：具体族在泛名前。
/// 窗口 ≥150k 时撞限闸行为等价（cap=min(窗×0.8, 120k)），表只为小窗口
/// 模型（deepseek 64k、gpt-3.5 16k）兜底精度。
const MODEL_META_TABLE: &[(&str, u64, u64)] = &[
    ("deepseek", 65_536, 8_192),
    ("qwen3-coder", 262_144, 65_536),
    ("qwen", 131_072, 8_192),
    ("glm", 131_072, 8_192),
    ("kimi", 131_072, 16_384),
    ("moonshot", 131_072, 16_384),
    ("claude", 200_000, 8_192),
    ("gpt-4o", 131_072, 16_384),
    ("gpt-4.1", 1_000_000, 32_768),
    ("gpt-5", 400_000, 32_768),
    ("gpt-3.5", 16_385, 4_096),
    ("o1", 200_000, 32_768),
    ("o3", 200_000, 32_768),
    ("o4", 200_000, 32_768),
    ("gemini", 1_000_000, 65_536),
    ("minimax", 200_000, 8_192),
    ("llama", 131_072, 8_192),
    ("mistral", 131_072, 8_192),
];

fn meta_lookup(id: &str) -> Option<&'static (&'static str, u64, u64)> {
    let l = id.to_lowercase();
    MODEL_META_TABLE.iter().find(|(k, _, _)| l.contains(k))
}

/// 模型 id → 上下文窗口 tok（内置前缀表）。不识别 → None。
pub fn infer_window(id: &str) -> Option<u64> {
    meta_lookup(id).map(|(_, w, _)| *w)
}

/// 模型 id → 输出上限 tok（内置前缀表）。不识别 → None。
pub fn infer_max_output(id: &str) -> Option<u64> {
    meta_lookup(id).map(|(_, _, o)| *o)
}

/// 目录条目优先、前缀表兜底（票 02 解析链的单点实现）。
pub fn window_of(def: &ProviderDef, model: &str) -> Option<u64> {
    def.models
        .iter()
        .find(|m| m.id == model)
        .and_then(|m| m.context_window)
        .or_else(|| infer_window(model))
}

/// 同上，输出上限。
pub fn max_output_of(def: &ProviderDef, model: &str) -> Option<u64> {
    def.models
        .iter()
        .find(|m| m.id == model)
        .and_then(|m| m.max_output)
        .or_else(|| infer_max_output(model))
}

/// 槽位 → 窗口（票 02 命名接缝）：load → resolve_slot（default 兜底）→
/// provider → 条目/前缀表，任一环断 → None（调用方回落旧默认）。
/// 只读查询口；实例路径已在 make_provider 挂表时解析进 ModelMeta。
pub fn window_for_slot(slot: &str) -> Option<u64> {
    let doc = load().ok()?;
    let b = resolve_slot(&doc.slots, slot)?;
    let p = doc.providers.iter().find(|p| p.id == b.provider_id)?;
    window_of(p, &b.model)
}

/// 同上，输出上限。
pub fn max_output_for_slot(slot: &str) -> Option<u64> {
    let doc = load().ok()?;
    let b = resolve_slot(&doc.slots, slot)?;
    let p = doc.providers.iter().find(|p| p.id == b.provider_id)?;
    max_output_of(p, &b.model)
}

#[cfg(test)]
/// 测试用进程级锁：providers.json 路径走 `HEXAGON_PROVIDERS_PATH` 环境变量
/// （进程全局），并行测试互踩——一方 set 夹在另一方 remove 之间就丢配置。
/// 所有碰该环境变量的测试先拿这把锁（provider.rs 的 server_tool_tests 同款）。
#[cfg(test)]
pub(crate) static PROVIDERS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let _env = PROVIDERS_ENV_LOCK.lock().unwrap();
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

    /// 票 02（context-window / ADR 0068）：模型窗口/输出上限的解析链——
    /// 目录条目显式值 > 内置前缀表 > None（调用方回落旧默认，不误判小窗口）。
    #[test]
    fn model_meta_entry_beats_table_beats_none() {
        let mut d = def("p");
        // 条目显式值优先
        d.models.push(ModelEntry {
            id: "custom/m".into(),
            context_window: Some(8_192),
            max_output: Some(512),
            ..ModelEntry::default()
        });
        assert_eq!(window_of(&d, "custom/m"), Some(8_192));
        assert_eq!(max_output_of(&d, "custom/m"), Some(512));
        // 条目留空 → 前缀表兜底（deepseek 64k 窗 / 8k 出）
        d.models.push(ModelEntry {
            id: "deepseek-chat".into(),
            ..ModelEntry::default()
        });
        assert_eq!(window_of(&d, "deepseek-chat"), Some(65_536));
        assert_eq!(max_output_of(&d, "deepseek-chat"), Some(8_192));
        // 不在目录也不在表 → None（回落旧默认，不是 0）
        assert_eq!(window_of(&d, "totally-unknown-xyz"), None);
        assert_eq!(max_output_of(&d, "totally-unknown-xyz"), None);
    }

    /// 票 02：槽位级解析链逐级断点——未绑槽走 default 兜底照常解析；
    /// 绑定指向不存在的供应商 → None；无绑定无 default → None。
    #[test]
    fn window_for_slot_walks_binding_and_default_fallback() {
        let _env = PROVIDERS_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _g = PathGuard::set(dir.path());
        let mut doc = ProviderDoc {
            providers: vec![def("p")],
            ..ProviderDoc::default()
        };
        doc.providers[0].models.push(ModelEntry {
            id: "deepseek-chat".into(),
            ..ModelEntry::default()
        });
        doc.slots.insert(
            "default".into(),
            SlotBinding {
                provider_id: "p".into(),
                model: "deepseek-chat".into(),
            },
        );
        std::fs::write(
            dir.path().join("providers.json"),
            serde_json::to_string(&doc).unwrap(),
        )
        .unwrap();
        // 「chat」未绑 → resolve_slot default 兜底 → deepseek 前缀表
        assert_eq!(window_for_slot("chat"), Some(65_536));
        assert_eq!(max_output_for_slot("chat"), Some(8_192));
        // 绑定指向不存在的供应商 → 链断 None（不回落 default——
        // resolve_slot 只在「槽未绑」时兜底，绑定本身坏是另一回事）
        doc.slots.insert(
            "chat".into(),
            SlotBinding {
                provider_id: "ghost".into(),
                model: "x".into(),
            },
        );
        std::fs::write(
            dir.path().join("providers.json"),
            serde_json::to_string(&doc).unwrap(),
        )
        .unwrap();
        assert_eq!(window_for_slot("chat"), None);
    }

    /// 前缀表按 id 子串兜底：常见族有值；qwen3-coder 比泛 qwen 先匹；
    /// 不识别 → None。
    #[test]
    fn infer_window_covers_fleet_and_stays_conservative() {
        assert_eq!(infer_window("qwen3-coder-plus"), Some(262_144));
        assert_eq!(infer_window("qwen3.7-plus"), Some(131_072));
        assert_eq!(infer_window("glm-5"), Some(131_072));
        assert_eq!(infer_window("deepseek-v3.2"), Some(65_536));
        assert_eq!(infer_window("claude-sonnet-4-5"), Some(200_000));
        assert_eq!(infer_window("gpt-3.5-turbo"), Some(16_385));
        assert_eq!(infer_window("mystery-model"), None);
    }

    /// 旧 providers.json（无新字段）照常加载——serde default 兼容。
    #[test]
    fn legacy_model_entry_without_meta_loads() {
        let v: ModelEntry = serde_json::from_str(r#"{"id":"m","caps":["tools"]}"#).unwrap();
        assert_eq!(v.context_window, None);
        assert_eq!(v.max_output, None);
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
