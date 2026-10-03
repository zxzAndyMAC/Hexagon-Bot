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
/// caps 词表见 docs/glossary.html：对话能力、输入/输出模态、嵌入/重排等目录标记。
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
    /// 内置前缀表兜底填，未识别留空 → 撞限闸回落 1M 全局上限，
    /// 设置页提示手填。大窗口不放开吃满——1M 仍是刻意纪律上限。
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

/// 三个起草槽。没绑就经 [`resolve_slot`] 落到 default。
pub const ROLE_DRAFT_SLOT: &str = "role_draft";
pub const BRIEF_SLOT: &str = "brief";
pub const FLOW_DRAFT_SLOT: &str = "flow_draft";
/// 提示词页参考译文（prompt-engineering 票 11）。宿主级槽，没绑落 default。
pub const TRANSLATE_SLOT: &str = "translate";
/// Jev。不走 default 回退，见 [`resolve_exact`]。
pub const JEV_SLOT: &str = "jev";

/// 槽位 → 供应商+模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SlotBinding {
    pub provider_id: String,
    pub model: String,
}

/// web 搜索摘要槽（code-search-and-subagent 票 03）：可选——未配置时
/// web_search 工具回报「槽未配置」。`backend` 目前认 `brave` 与 `custom`
/// （自建元搜索的预留位，endpoint 必填）；凭据名 `search/<backend>`
/// 存钥匙串，不落本文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SearchCfg {
    pub backend: String,
    #[serde(default)]
    pub endpoint: Option<String>,
}

/// providers.json 文档。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ProviderDoc {
    #[serde(default)]
    pub providers: Vec<ProviderDef>,
    #[serde(default)]
    pub slots: HashMap<String, SlotBinding>,
    #[serde(default)]
    pub search: Option<SearchCfg>,
}

fn providers_path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = FIXTURE_PATH.with(|p| p.borrow().clone()) {
        return path;
    }
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
    #[cfg(test)]
    if let Some(doc) = FIXTURE_DOCUMENT.with(|d| d.borrow().clone()) {
        return Ok(doc);
    }

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

/// 设置/清除 web 搜索摘要槽（票 03）。后端名空串=清除；
/// 凭据不归本函数管（壳层经 provider_admin::save_search 走凭据库）。
pub fn set_search(search: Option<SearchCfg>) -> Result<(), ProvidersError> {
    let mut doc = load()?;
    doc.search = search;
    write_doc(&doc)
}

/// 槽位回退链（全仓唯一实现，arch-review 票 01 / 诊断卡 D13）：
/// 绑定槽缺失时回退 `default` 槽。调用方不得再手写 `.or_else(get("default"))`。
pub fn resolve_slot<'a, V>(map: &'a HashMap<String, V>, slot: &str) -> Option<&'a V> {
    map.get(slot).or_else(|| map.get("default"))
}

/// 精确绑定。Jev 用这个：没绑就是空，不落到 default，也不落到某个 Agent 的模型槽。
/// 回退链仍然只有 [`resolve_slot`] 一处。
pub fn resolve_exact<'a, V>(map: &'a HashMap<String, V>, slot: &str) -> Option<&'a V> {
    map.get(slot)
}

/// 「这次解析是不是落到了 default」——诊断记录用（diagnostic-records 票 03）。
/// 与 [`resolve_slot`] 同一行链语义：链若加跳这里跟着改，调用点不许各自
/// 用 contains_key 反推（D13：回退语义只在本文件）。resolve_slot 失败
/// （default 也没绑）不算回退，是错误。
pub fn fell_back_to_default<V>(map: &HashMap<String, V>, slot: &str) -> bool {
    !map.contains_key(slot) && map.contains_key("default")
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
        .map_err(|_| ProvidersError::Http("provider request or response failed".into()))?;
    let v: Value = resp
        .body_mut()
        .read_json()
        .map_err(|_| ProvidersError::Http("provider request or response failed".into()))?;
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
/// 返回 ModelEntry（id + 分组 + 推断能力）；4xx/网络错 → 安全 Http 原因。
/// Reliability 22: ureq BadUri used to echo authentication URLs into UI errors.
/// Never return raw transport/parser errors, which can contain credentials.
pub fn fetch_models(def: &ProviderDef, key: &str) -> Result<Vec<ModelEntry>, ProvidersError> {
    let base = def.base_url.trim_end_matches('/');
    // Gemini's OpenAI-compatible list omits native metadata. Stay on the exact
    // configured official host; never redirect a proxy's key to another service.
    let google = url::Url::parse(base).ok().is_some_and(|u| {
        u.host_str() == Some("generativelanguage.googleapis.com") && u.path() == "/v1beta/openai"
    });
    let url = match def.kind {
        ProviderKind::OpenAi if google => format!("{}/models", base.trim_end_matches("/openai")),
        ProviderKind::Anthropic => format!("{base}/v1/models"),
        ProviderKind::OpenAi => format!("{base}/models"),
        // Jev 没有 /models。用一道最小选择题确认钥匙，模型名以响应为准。
        ProviderKind::Jev => return fetch_jev_models(def, key),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let mut models = Vec::new();
    let mut after: Option<String> = None;
    let mut seen = std::collections::HashSet::new();
    loop {
        let req = agent.get(&url);
        let req = if let Some(cursor) = &after {
            req.query(if google { "pageToken" } else { "after_id" }, cursor)
        } else {
            req
        };
        let req = if url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .as_deref()
            == Some("openrouter.ai")
        {
            req.query("output_modalities", "all")
        } else {
            req
        };
        let req = match def.kind {
            ProviderKind::Anthropic => req
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
            ProviderKind::OpenAi if google => req.header("x-goog-api-key", key),
            ProviderKind::OpenAi => req.header("Authorization", &format!("Bearer {key}")),
            ProviderKind::Jev => unreachable!("jev returns before the models request"),
        };
        let mut resp = req
            .call()
            .map_err(|_| ProvidersError::Http("provider request or response failed".into()))?;
        let v: Value = resp
            .body_mut()
            .read_json()
            .map_err(|_| ProvidersError::Http("provider request or response failed".into()))?;
        let page = v
            .get("data")
            .or_else(|| v.get("models"))
            .unwrap_or(&v)
            .as_array()
            .ok_or_else(|| ProvidersError::Http("invalid model catalog".into()))?;
        models.extend(page.iter().filter_map(catalog_model));
        let next = if google {
            match v["nextPageToken"].as_str().filter(|s| !s.is_empty()) {
                Some(next) => next,
                None => break,
            }
        } else {
            if !matches!(def.kind, ProviderKind::Anthropic) || v["has_more"].as_bool() != Some(true)
            {
                break;
            }
            v["last_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ProvidersError::Http("invalid model catalog cursor".into()))?
        };
        if !seen.insert(next.to_owned()) || seen.len() > 100 {
            return Err(ProvidersError::Http(
                "model catalog pagination did not finish".into(),
            ));
        }
        after = Some(next.to_owned());
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

// Owner 2026-10-01: /models formerly discarded every field except id, incorrectly
// granting tools to embedding/rerank models. Read documented Anthropic/OpenRouter
// metadata first. A false negative needs manual correction; a false positive can
// expose unsupported tools. Explicit false/empty metadata therefore beats guesses.
// Sources: platform.claude.com/docs/en/api/models/list;
// openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties.
fn catalog_model(v: &Value) -> Option<ModelEntry> {
    let id = v["id"]
        .as_str()
        .or_else(|| {
            v.get("supportedGenerationMethods")?;
            v["name"]
                .as_str()
                .map(|name| name.strip_prefix("models/").unwrap_or(name))
        })?
        .trim();
    if id.is_empty() {
        return None;
    }
    let mut caps = infer_caps(id);
    let architecture = v.get("architecture").unwrap_or(v);
    let mut set = |cap: &str, supported: bool| {
        caps.retain(|c| c != cap);
        if supported {
            caps.push(cap.into());
        }
    };
    for (field, mappings) in [
        (
            "input_modalities",
            &[
                ("image", "vision"),
                ("audio", "audio_input"),
                ("video", "video_input"),
                ("file", "file_input"),
            ][..],
        ),
        (
            "output_modalities",
            &[
                ("image", "image_generation"),
                ("audio", "audio_output"),
                ("speech", "audio_output"),
                ("video", "video_generation"),
                ("embeddings", "embedding"),
                ("rerank", "rerank"),
                ("transcription", "audio_input"),
            ][..],
        ),
    ] {
        if let Some(values) = architecture[field].as_array() {
            // Multiple wire modalities may map to one badge (audio/speech).
            for (_, cap) in mappings {
                set(
                    cap,
                    mappings.iter().any(|(wire, mapped)| {
                        mapped == cap && values.iter().any(|x| x.as_str() == Some(wire))
                    }),
                );
            }
        }
    }
    if architecture["output_modalities"]
        .as_array()
        .is_some_and(|a| !a.iter().any(|m| m.as_str() == Some("text")))
    {
        set("tools", false);
    }
    if let Some(params) = v["supported_parameters"].as_array() {
        for (cap, names) in [
            ("tools", &["tools", "tool_choice"][..]),
            ("reasoning", &["reasoning", "reasoning_effort"][..]),
            ("structured_output", &["structured_outputs"][..]),
        ] {
            set(
                cap,
                params
                    .iter()
                    .any(|p| p.as_str().is_some_and(|p| names.contains(&p))),
            );
        }
    }
    for (wire, cap) in [
        ("image_input", "vision"),
        ("thinking", "reasoning"),
        ("structured_outputs", "structured_output"),
        ("pdf_input", "file_input"),
        ("batch", "batch"),
        ("citations", "citations"),
        ("code_execution", "code_execution"),
    ] {
        if let Some(supported) = v["capabilities"][wire]["supported"].as_bool() {
            set(cap, supported);
        }
    }
    if let Some(params) = v["supported_parameters"].as_array() {
        set(
            "json_mode",
            params.iter().any(|p| p.as_str() == Some("response_format")),
        );
    }
    // Mistral uses boolean capability fields; Kimi uses top-level supports_*.
    for (wire, cap) in [
        ("function_calling", "tools"),
        ("vision", "vision"),
        ("classification", "classification"),
        ("moderation", "moderation"),
        ("reasoning", "reasoning"),
        ("ocr", "ocr"),
        ("completion_chat", "text_generation"),
        ("audio_transcription", "audio_input"),
        ("audio_speech", "audio_output"),
    ] {
        if let Some(supported) = v["capabilities"][wire].as_bool() {
            set(cap, supported);
        }
    }
    for (wire, cap) in [
        ("supports_image_in", "vision"),
        ("supports_video_in", "video_input"),
        ("supports_reasoning", "reasoning"),
    ] {
        if let Some(supported) = v[wire].as_bool() {
            set(cap, supported);
        }
    }
    // DeepSeek /models: effort levels exclude 'none' and apply to thinking mode.
    if let Some(levels) = v["effort"]["supported_levels"].as_array() {
        set(
            "reasoning",
            levels
                .iter()
                .any(|x| x.as_str().is_some_and(|s| !s.is_empty() && s != "none")),
        );
    }
    if let Some(thinking) = v["thinking"].as_bool() {
        set("reasoning", thinking);
    }
    if let Some(methods) = v["supportedGenerationMethods"].as_array() {
        let has = |method| methods.iter().any(|m| m.as_str() == Some(method));
        set("embedding", has("embedContent"));
        set("text_generation", has("generateContent"));
        if !has("generateContent") {
            set("tools", false);
        }
    }
    // Together / Qianfan expose task type even when output modalities are empty.
    if let Some(kind) = v["type"].as_str() {
        let cap = match kind {
            "embedding" | "embeddings" => Some("embedding"),
            "rerank" => Some("rerank"),
            "image" | "text2image" => Some("image_generation"),
            "image2text" => Some("vision"),
            "moderation" => Some("moderation"),
            "chat" | "language" | "code" => Some("text_generation"),
            _ => None,
        };
        if let Some(cap) = cap {
            set(cap, true);
        }
        if matches!(
            kind,
            "embedding" | "embeddings" | "rerank" | "image" | "text2image" | "moderation"
        ) {
            set("tools", false);
        }
    }
    let positive = |v: &Value| v.as_u64().filter(|n| *n > 0);
    Some(ModelEntry {
        id: id.into(),
        group: Some(group_of(id)),
        caps,
        name: v["display_name"]
            .as_str()
            .or_else(|| v["displayName"].as_str())
            .or_else(|| v["name"].as_str())
            .map(String::from),
        context_window: positive(&v["inputTokenLimit"])
            .or_else(|| positive(&v["context_window"]))
            .or_else(|| positive(&v["max_context_length"]))
            .or_else(|| positive(&v["max_input_tokens"]))
            .or_else(|| positive(&v["context_length"]))
            .or_else(|| infer_window(id)),
        max_output: positive(&v["outputTokenLimit"])
            .or_else(|| positive(&v["max_completion_tokens"]))
            .or_else(|| positive(&v["max_output_tokens"]))
            .or_else(|| positive(&v["max_tokens"]))
            .or_else(|| positive(&v["top_provider"]["max_completion_tokens"]))
            .or_else(|| infer_max_output(id)),
    })
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
    // Catalog-only modalities do not imply that Hexagon has an executor for them.
    for (cap, patterns) in [
        ("embedding", &["embed", "bge-m3"][..]),
        ("rerank", &["rerank"][..]),
        (
            "image_generation",
            &["dall-e", "gpt-image", "stable-diffusion", "flux-"][..],
        ),
        ("audio_input", &["whisper", "transcribe"][..]),
        ("audio_output", &["tts"][..]),
        ("video_generation", &["sora", "veo-"][..]),
        ("moderation", &["moderation"][..]),
    ] {
        if patterns.iter().any(|p| l.contains(p)) {
            caps.push(cap.into());
        }
    }
    // 工具调用：现代对话模型默认带，只排除明确的非对话模型
    if ![
        "embed",
        "bge-m3",
        "rerank",
        "whisper",
        "transcribe",
        "tts",
        "dall",
        "moderation",
        "audio",
        "gpt-image",
        "stable-diffusion",
        "flux-",
        "sora",
        "veo-",
    ]
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
/// 窗口 ≥1.25M 时撞限闸行为等价（cap=min(窗×0.8, 1M)），表只为小窗口
/// 模型（deepseek 64k、gpt-3.5 16k）兜底精度。
const MODEL_META_TABLE: &[(&str, u64, u64)] = &[
    // Live acceptance 2026-10-01: generic DeepSeek limits caused V4 compaction loops.
    // Official /models gives 1,048,576 context; 32K is Hexagon's fallback request
    // budget, not the service ceiling/default: api-docs.deepseek.com/api/list-models/
    ("deepseek-flash", 1_048_576, 32_768),
    ("deepseek-v4", 1_048_576, 32_768),
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
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;

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

    #[test]
    fn concurrent_provider_files_do_not_change_another_threads_frozen_inputs() {
        // Evaluation22: the old process-global path made unrelated evaluations
        // intermittently fail "configuration drift" during provider CRUD tests.
        let parent = tempfile::tempdir().unwrap();
        let _parent = fixture_path(parent.path().join("providers.json"));
        save_provider(&def("parent")).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let threads: Vec<_> = ["left", "right"]
            .into_iter()
            .map(|id| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let home = tempfile::tempdir().unwrap();
                    let _path = fixture_path(home.path().join("providers.json"));
                    save_provider(&def(id)).unwrap();
                    barrier.wait();
                    assert_eq!(load().unwrap().providers[0].id, id);
                    {
                        let nested = tempfile::tempdir().unwrap();
                        let _nested = fixture_path(nested.path().join("providers.json"));
                        assert!(load().unwrap().providers.is_empty());
                    }
                    assert_eq!(load().unwrap().providers[0].id, id);
                })
            })
            .collect();
        barrier.wait();
        assert_eq!(load().unwrap().providers[0].id, "parent");
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(load().unwrap().providers[0].id, "parent");
    }

    /// 供应商 CRUD + 槽位绑定生命周期 + 就绪判定（绑定/启用/key 三要件）。
    #[test]
    fn provider_def_and_binding_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let _g = fixture_path(dir.path().join("providers.json"));
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
        let dir = tempfile::tempdir().unwrap();
        let _g = fixture_path(dir.path().join("providers.json"));
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
        // Live acceptance 2026-10-01: V4 inherited V3 limits and repeatedly compacted.
        for model in ["deepseek-v4-pro", "deepseek-v4-flash"] {
            assert_eq!(infer_window(model), Some(1_048_576));
            assert_eq!(infer_max_output(model), Some(32_768));
        }
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

    #[test]
    fn anthropic_catalog_follows_pages_and_keeps_metadata() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for page in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let n = socket.read(&mut bytes).unwrap();
                let request = String::from_utf8_lossy(&bytes[..n]);
                assert!(request.starts_with("GET /v1/models"));
                if page == 1 {
                    assert!(request.contains("after_id=model-0"));
                }
                let body = serde_json::json!({"data":[{"id":format!("model-{page}"),
                    "max_input_tokens":123456,"max_tokens":8192}],
                    "has_more":page == 0,"last_id":format!("model-{page}")})
                .to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let def = ProviderDef {
            id: "test".into(),
            name: "test".into(),
            kind: ProviderKind::Anthropic,
            base_url: format!("http://{address}"),
            models: vec![],
            enabled: true,
        };
        let models = fetch_models(&def, "synthetic").unwrap();
        server.join().unwrap();
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|m| m.context_window == Some(123456)));
    }

    #[test]
    fn catalog_metadata_overrides_name_guesses() {
        let m = catalog_model(&serde_json::json!({
            "id": "claude-reasoning", "name": "Catalog name", "context_length": 123456,
            "top_provider": {"max_completion_tokens": 4321},
            "architecture": {"input_modalities": ["text"], "output_modalities": ["embeddings"]},
            "supported_parameters": []
        }))
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("Catalog name"));
        assert_eq!(m.context_window, Some(123456));
        assert_eq!(m.max_output, Some(4321));
        assert!(m.caps.contains(&"embedding".into()));
        for cap in ["tools", "vision", "reasoning"] {
            assert!(!m.caps.contains(&cap.into()));
        }
        let m = catalog_model(&serde_json::json!({
            "id": "claude-sonnet-4", "display_name": "Claude", "max_input_tokens": 1000000,
            "max_tokens": 64000, "capabilities": {
                "image_input": {"supported": false}, "thinking": {"supported": true},
                "structured_outputs": {"supported": true}, "pdf_input": {"supported": true}
            }
        }))
        .unwrap();
        assert!(!m.caps.contains(&"vision".into()));
        assert!(m.caps.contains(&"reasoning".into()));
        assert!(m.caps.contains(&"structured_output".into()));
        assert!(m.caps.contains(&"file_input".into()));
        assert_eq!(m.context_window, Some(1000000));
    }

    #[test]
    fn direct_vendor_catalogs_supply_explicit_capabilities_and_limits() {
        for (value, cap, window) in [
            (
                serde_json::json!({"id":"deepseek-flash", "context_window":1048576, "max_output_tokens":393216,
                "input_modalities":["text","image"], "output_modalities":["text"], "effort":{"supported_levels":["high"]}}),
                "reasoning",
                1048576,
            ),
            (
                serde_json::json!({"id":"kimi-test", "context_length":256000, "supports_image_in":true,
                "supports_video_in":true, "supports_reasoning":false}),
                "video_input",
                256000,
            ),
            (
                serde_json::json!({"id":"mistral-test", "max_context_length":128000,
                "capabilities":{"function_calling":false,"vision":true}}),
                "vision",
                128000,
            ),
        ] {
            let model = catalog_model(&value).unwrap();
            assert!(model.caps.contains(&cap.to_string()));
            assert_eq!(model.context_window, Some(window));
            if model.id == "mistral-test" {
                assert!(!model.caps.contains(&"tools".into()));
            }
        }
    }

    #[test]
    fn google_and_specialist_models_are_not_mistaken_for_tool_agents() {
        let google = catalog_model(&serde_json::json!({"name":"models/gemini-embedding-test",
            "displayName":"Embedding", "inputTokenLimit":8192, "outputTokenLimit":1,
            "supportedGenerationMethods":["embedContent"], "thinking":false}))
        .unwrap();
        assert_eq!(google.id, "gemini-embedding-test");
        assert_eq!(google.context_window, Some(8192));
        assert!(google.caps.contains(&"embedding".into()));
        assert!(!google.caps.contains(&"tools".into()));
        for kind in [
            "embedding",
            "embeddings",
            "rerank",
            "image",
            "text2image",
            "moderation",
        ] {
            let model =
                catalog_model(&serde_json::json!({"id":"specialist", "type":kind})).unwrap();
            assert!(!model.caps.contains(&"tools".into()), "{kind}");
        }
    }

    proptest::proptest! {
        #[test]
        fn explicit_capability_denial_always_beats_name(id in "[a-z-]{0,40}") {
            let m = catalog_model(&serde_json::json!({
                "id": format!("claude-vision-thinking-{id}"),
                "capabilities": {"image_input": {"supported": false}, "thinking": {"supported": false}},
                "supported_parameters": []
            })).unwrap();
            proptest::prop_assert!(!m.caps.iter().any(|c| matches!(c.as_str(), "vision" | "reasoning" | "tools")));
        }
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

// Evaluation 17: isolate supplier-boundary fixtures per test thread. Mutating
// process environment raced concurrent frozen configuration checks.
#[cfg(test)]
thread_local! { static FIXTURE_DOCUMENT:std::cell::RefCell<Option<ProviderDoc>>=const {std::cell::RefCell::new(None)}; }
#[cfg(test)]
pub(crate) struct FixtureDocument(Option<ProviderDoc>);
#[cfg(test)]
impl Drop for FixtureDocument {
    fn drop(&mut self) {
        FIXTURE_DOCUMENT.with(|d| *d.borrow_mut() = self.0.take());
    }
}
#[cfg(test)]
pub(crate) fn fixture_document(doc: ProviderDoc) -> FixtureDocument {
    FixtureDocument(FIXTURE_DOCUMENT.with(|d| d.replace(Some(doc))))
}

// Evaluation22 regression: writer-only ENV_LOCK did not protect concurrent
// evaluation readers. Temporary provider documents appeared as real runtime
// drift. Keep test paths thread-local; product environment resolution is unchanged.
#[cfg(test)]
thread_local! { static FIXTURE_PATH:std::cell::RefCell<Option<PathBuf>>=const {std::cell::RefCell::new(None)}; }
#[cfg(test)]
pub(crate) struct FixturePath {
    previous: Option<PathBuf>,
    same_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
#[cfg(test)]
impl Drop for FixturePath {
    fn drop(&mut self) {
        FIXTURE_PATH.with(|p| *p.borrow_mut() = self.previous.take());
    }
}
#[cfg(test)]
pub(crate) fn fixture_path(path: PathBuf) -> FixturePath {
    FixturePath {
        previous: FIXTURE_PATH.with(|p| p.replace(Some(path))),
        same_thread: std::marker::PhantomData,
    }
}
