//! 内置服务商目录（owner-followups-1015 第 12 项，2026-10-18 负责人裁决；票 22）。
//!
//! 出处：内置目录原先只有手工维护的 14 条（`provider_presets`）。负责人追加：引入固定快照的
//! 服务商目录，收录其中「有 OpenAI Chat Completions 或 Anthropic Messages 端点、且用 API Key」
//! 的服务商（实收 56 家），手工 14 条覆盖同名条目。
//!
//! 快照：Cherry Studio `packages/provider-registry/data/providers.json` @ [`SNAPSHOT_COMMIT`]，
//! 原样存于 `data/provider-catalog/providers.json`（MIT，原文 LICENSE 同目录），测试校验
//! [`SNAPSHOT_SHA256`]。`extras.json` 是同一提交派生的图标键与示例模型（上游示例模型在另一份
//! models 表里，这里只取每家至多三个作说明，不预填）。目录升级 = 换这两个文件 + 改 sha256，
//! `final_request_urls_for_all_56` 对照表会逼着逐家核对最终请求地址。
//!
//! 转换层（本模块唯一职责）：快照条目 → 预设。
//! - 排除：OAuth / 外部 CLI 登录、仅 Responses、仅 Gemini、仅图像（Hexagon 只会说 Chat Completions
//!   与 Anthropic Messages，也只存 API Key）；另按 2026-10-18 协议会话核实点名排除 `aws-bedrock`
//!   （需区域签名）、`vertexai`（需服务账号）、`azure-openai`（钥匙在 `api-key` 头）、`copilot`
//!   （需 OAuth）——这四家快照里登记了 Chat / Messages 端点，按端点判会被误收。由
//!   `.scratch/provider-protocols/` 的协议票逐个解除。
//! - 选端点：优先 OpenAI Chat Completions，其次 Anthropic Messages。
//! - 写死最终请求基址：快照地址按上游客户端的口径（末尾 `/` = 原样用；否则 OpenAI 兼容补 `/v1`，
//!   Anthropic SDK 补 `/v1/messages`）算出最终请求地址，再反推一个「过 `provider_url::normalize`
//!   不变、`request_url` 恰好落到该地址」的存储值。否决的替代：直接用快照地址——Perplexity
//!   `https://api.perplexity.ai/` 保存时会被规范化补 `/v1`（404），且上游与 Hexagon 的补段规则
//!   并不相同，换快照时没人会逐家重算。
//! - 覆盖表 [`URL_OVERRIDES`]：上游地址属于其专用适配族、不是 Chat Completions 地址的条目。
//! - 无公共地址（GPUStack 等）→ 地址留空，由负责人自填。
//!
//! 判定代价：误收（把不能用的端点收进来）= 负责人选中后检测失败、白配一次；误排 = 负责人走
//! 「添加」手填一次。两边都是一次人工，偏向排除——点名排除表只增不减，直到协议票解除。

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::provider::ProviderKind;
use crate::provider_url;

/// 快照来源提交（Cherry Studio）。
pub const SNAPSHOT_COMMIT: &str = "9d1bf05fe2c78231d43e383118d5131f8ad5f49f";
/// `data/provider-catalog/providers.json` 的 sha256（逐字节，测试锁住）。
pub const SNAPSHOT_SHA256: &str =
    "fb00fbd89fc661e05f6645d943c5c50dd9110ad2c6857343dd5324941d5a4e34";

pub(crate) const SNAPSHOT: &str = include_str!("../data/provider-catalog/providers.json");
const EXTRAS: &str = include_str!("../data/provider-catalog/extras.json");

const CHAT: &str = "openai-chat-completions";
const MESSAGES: &str = "anthropic-messages";

/// 点名排除（2026-10-18 协议会话核实，见模块注释）：(id, 原因)。
const PINNED_EXCLUSIONS: &[(&str, &str)] = &[
    ("aws-bedrock", "region-signed requests (SigV4)"),
    ("vertexai", "service-account auth"),
    ("azure-openai", "key goes in the api-key header"),
    ("copilot", "OAuth device login"),
];

/// 覆盖表：快照地址不是 Chat Completions 根的条目 → Hexagon 存储基址。
/// - `gateway`：上游 `…/v1/ai` 是 Vercel AI SDK 网关协议的根，OpenAI 兼容端点在
///   `https://ai-gateway.vercel.sh/v1/chat/completions`（vercel.com/docs/ai-gateway openai-compat）。
const URL_OVERRIDES: &[(&str, &str)] = &[("gateway", "https://ai-gateway.vercel.sh/v1")];

/// 排除原因（只给测试与诊断看）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exclusion {
    /// 登录方式不是 API Key（OAuth / 外部 CLI）。
    Auth(String),
    /// 没有 Chat Completions / Messages 端点；值为它有的端点（如仅 Responses / Gemini / 图像）。
    NoChatEndpoint(Vec<String>),
    /// 点名排除，见 [`PINNED_EXCLUSIONS`]。
    Pinned(&'static str),
}

#[derive(Debug, Deserialize)]
struct Snapshot {
    providers: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    endpoint_configs: BTreeMap<String, Endpoint>,
    #[serde(default)]
    auth_methods: Option<Vec<String>>,
    #[serde(default)]
    metadata: Option<Metadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Endpoint {
    #[serde(default)]
    base_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Metadata {
    #[serde(default)]
    website: Option<Website>,
}

#[derive(Debug, Default, Deserialize)]
struct Website {
    #[serde(default)]
    official: Option<String>,
    #[serde(default)]
    docs: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Extra {
    #[serde(default)]
    icon_key: String,
    #[serde(default)]
    sample_models: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Extras {
    providers: BTreeMap<String, Extra>,
}

/// 目录条目的说明性元数据（官网 / 文档 / 图标键 / 示例模型）。手工预设覆盖同名条目时也借用。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CatalogMeta {
    pub icon_key: String,
    pub website: String,
    pub docs: String,
    pub sample_models: Vec<String>,
}

/// 转换后的目录条目（尚未套手工覆盖）。
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    /// 存储基址：过 `normalize` 不变，`request_url` 落到最终请求地址；无公共地址时为空。
    pub base_url: String,
    pub meta: CatalogMeta,
}

fn snapshot() -> Vec<Entry> {
    // 快照随二进制编译进来、由测试锁住 sha256 与解析——解析失败是构建期错误，不是运行时输入。
    serde_json::from_str::<Snapshot>(SNAPSHOT)
        .expect("embedded provider catalog snapshot parses (locked by tests)")
        .providers
}

fn extras() -> BTreeMap<String, Extra> {
    serde_json::from_str::<Extras>(EXTRAS)
        .expect("embedded provider catalog extras parse (locked by tests)")
        .providers
}

fn meta_of(entry: &Entry, extras: &BTreeMap<String, Extra>) -> CatalogMeta {
    let site = entry.metadata.as_ref().and_then(|m| m.website.as_ref());
    let extra = extras.get(&entry.id);
    CatalogMeta {
        icon_key: extra.map(|e| e.icon_key.clone()).unwrap_or_default(),
        website: site.and_then(|w| w.official.clone()).unwrap_or_default(),
        docs: site.and_then(|w| w.docs.clone()).unwrap_or_default(),
        sample_models: extra.map(|e| e.sample_models.clone()).unwrap_or_default(),
    }
}

/// 快照条目为什么不收；`None` = 收。
pub(crate) fn exclusion(entry: &Entry) -> Option<Exclusion> {
    if let Some((_, why)) = PINNED_EXCLUSIONS.iter().find(|(id, _)| *id == entry.id) {
        return Some(Exclusion::Pinned(why));
    }
    if let Some(m) = entry
        .auth_methods
        .as_ref()
        .and_then(|ms| ms.iter().find(|m| m.as_str() != "api-key"))
    {
        return Some(Exclusion::Auth(m.clone()));
    }
    let has = |k: &str| entry.endpoint_configs.contains_key(k);
    if !has(CHAT) && !has(MESSAGES) {
        return Some(Exclusion::NoChatEndpoint(
            entry.endpoint_configs.keys().cloned().collect(),
        ));
    }
    None
}

/// 上游客户端口径下的最终请求地址：末尾 `/` = 原样用（Cherry `formatApiHost`）；否则 OpenAI
/// 兼容含版本段就不补、不含补 `/v1`；Anthropic SDK 一律补 `/v1/messages`。
fn upstream_request_url(kind: &ProviderKind, url: &str) -> String {
    let as_is = url.ends_with('/');
    let base = url.trim().trim_end_matches('/');
    match kind {
        ProviderKind::Anthropic => format!("{base}/v1/messages"),
        _ if as_is => format!("{base}/chat/completions"),
        _ => {
            let stored = provider_url::normalize(&ProviderKind::OpenAi, base);
            provider_url::request_url(&ProviderKind::OpenAi, &stored)
        }
    }
}

/// 最终请求地址 → 最短的存储值 X：`normalize(X) == X` 且 `request_url(X) == target`。
/// 候选从短到长（主机根 → 去端点尾 → 完整端点）；完整端点按规则①原样，必然命中。
pub fn fit_base(kind: &ProviderKind, target: &str) -> String {
    let mut candidates = Vec::new();
    if let Some(root) = target.strip_suffix("/v1/messages") {
        candidates.push(root);
    }
    for tail in ["/chat/completions", "/messages"] {
        if let Some(root) = target.strip_suffix(tail) {
            candidates.push(root);
        }
    }
    candidates.push(target);
    candidates
        .into_iter()
        .find(|c| {
            provider_url::normalize(kind, c) == *c && provider_url::request_url(kind, c) == target
        })
        .unwrap_or(target)
        .to_string()
}

fn convert(entry: &Entry, extras: &BTreeMap<String, Extra>) -> CatalogEntry {
    let (kind, key) = if entry.endpoint_configs.contains_key(CHAT) {
        (ProviderKind::OpenAi, CHAT)
    } else {
        (ProviderKind::Anthropic, MESSAGES)
    };
    let upstream = entry
        .endpoint_configs
        .get(key)
        .and_then(|e| e.base_url.clone())
        .unwrap_or_default();
    let base_url = match URL_OVERRIDES.iter().find(|(id, _)| *id == entry.id) {
        Some((_, url)) => url.to_string(),
        None if upstream.trim().is_empty() => String::new(),
        None => fit_base(&kind, &upstream_request_url(&kind, &upstream)),
    };
    CatalogEntry {
        id: entry.id.clone(),
        name: entry.name.clone(),
        kind,
        base_url,
        meta: meta_of(entry, extras),
    }
}

/// 收录的目录条目（快照顺序）。
pub fn entries() -> Vec<CatalogEntry> {
    let extras = extras();
    snapshot()
        .iter()
        .filter(|e| exclusion(e).is_none())
        .map(|e| convert(e, &extras))
        .collect()
}

/// 任一快照条目（含被排除的）的说明性元数据——手工预设覆盖同名条目时借图标键 / 官网 / 示例模型
/// （如手工 Gemini 走 OpenAI 兼容端点，目录里的 Gemini 条目因仅 Gemini 协议被排除，元数据仍可用）。
pub fn meta(id: &str) -> Option<CatalogMeta> {
    let extras = extras();
    snapshot()
        .iter()
        .find(|e| e.id == id)
        .map(|e| meta_of(e, &extras))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn snapshot_bytes_match_pinned_sha256() {
        let digest = Sha256::digest(SNAPSHOT.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex, SNAPSHOT_SHA256,
            "snapshot changed: update SNAPSHOT_SHA256 and re-check the 56-entry table"
        );
        assert_eq!(snapshot().len(), 66);
        // extras 必须与快照逐 id 对齐，换快照忘了换 extras 会在这里红。
        let ex = extras();
        for e in snapshot() {
            assert!(ex.contains_key(&e.id), "extras missing {}", e.id);
        }
    }

    #[test]
    fn exclusions_follow_the_ruling() {
        let excluded: BTreeMap<String, Exclusion> = snapshot()
            .iter()
            .filter_map(|e| exclusion(e).map(|x| (e.id.clone(), x)))
            .collect();
        let ids: Vec<&str> = excluded.keys().map(String::as_str).collect();
        assert_eq!(
            ids,
            [
                "aws-bedrock",
                "azure-openai",
                "claude-code",
                "comfyui",
                "copilot",
                "gemini",
                "grok-cli",
                "openai",
                "openai-codex",
                "vertexai",
            ]
        );
        assert_eq!(
            excluded["claude-code"],
            Exclusion::Auth("external-cli".into())
        );
        assert_eq!(excluded["openai-codex"], Exclusion::Auth("oauth".into()));
        assert_eq!(excluded["grok-cli"], Exclusion::Auth("oauth".into()));
        // 仅 Responses / 仅 Gemini / 仅图像
        assert_eq!(
            excluded["openai"],
            Exclusion::NoChatEndpoint(vec!["openai-responses".into()])
        );
        assert_eq!(
            excluded["gemini"],
            Exclusion::NoChatEndpoint(vec!["google-generate-content".into()])
        );
        assert_eq!(
            excluded["comfyui"],
            Exclusion::NoChatEndpoint(vec!["openai-image-generation".into()])
        );
        for id in ["aws-bedrock", "vertexai", "azure-openai", "copilot"] {
            assert!(matches!(excluded[id], Exclusion::Pinned(_)), "{id}");
        }
    }

    /// 56 家最终请求地址对照表（保存路径：normalize → request_url）。换快照时逐家核对。
    #[test]
    fn final_request_urls_for_all_56() {
        use ProviderKind::{Anthropic as A, OpenAi as O};
        let expected: &[(&str, ProviderKind, &str)] = &[
            (
                "cherryin",
                O,
                "https://open.cherryin.net/v1/chat/completions",
            ),
            (
                "silicon",
                O,
                "https://api.siliconflow.cn/v1/chat/completions",
            ),
            ("aihubmix", O, "https://aihubmix.com/v1/chat/completions"),
            ("ovms", O, "http://localhost:8000/v3/chat/completions"),
            ("ocoolai", O, "https://api.ocoolai.com/v1/chat/completions"),
            (
                "zhipu",
                O,
                "https://open.bigmodel.cn/api/paas/v4/chat/completions",
            ),
            (
                "deepseek",
                O,
                "https://api.deepseek.com/v1/chat/completions",
            ),
            (
                "alayanew",
                O,
                "https://deepseek.alayanew.com/v1/chat/completions",
            ),
            ("dmxapi", O, "https://www.dmxapi.cn/v1/chat/completions"),
            ("aionly", O, "https://api.aiionly.com/v1/chat/completions"),
            (
                "burncloud",
                O,
                "https://ai.burncloud.com/v1/chat/completions",
            ),
            ("302ai", O, "https://api.302.ai/v1/chat/completions"),
            (
                "lanyun",
                O,
                "https://maas-api.lanyun.net/v1/chat/completions",
            ),
            ("ph8", O, "https://ph8.co/v1/chat/completions"),
            (
                "sophnet",
                O,
                "https://www.sophnet.com/api/open-apis/v1/chat/completions",
            ),
            (
                "ppio",
                O,
                "https://api.ppinfra.com/v3/openai/chat/completions",
            ),
            ("qiniu", O, "https://api.qnaigc.com/v1/chat/completions"),
            (
                "openrouter",
                O,
                "https://openrouter.ai/api/v1/chat/completions",
            ),
            ("ollama", A, "http://localhost:11434/v1/messages"),
            (
                "radeon-cloud",
                O,
                "https://developer.amd.com.cn/radeon/v1/chat/completions",
            ),
            (
                "tokendance",
                O,
                "https://tokendance.space/gateway/v1/chat/completions",
            ),
            ("new-api", O, "http://localhost:3000/v1/chat/completions"),
            ("lmstudio", O, "http://localhost:1234/v1/chat/completions"),
            ("litellm", O, "http://localhost:4000/v1/chat/completions"),
            ("anthropic", A, "https://api.anthropic.com/v1/messages"),
            ("omlx", O, "http://localhost:8000/v1/chat/completions"),
            (
                "opencode",
                O,
                "https://opencode.ai/zen/go/v1/chat/completions",
            ),
            ("moonshot", O, "https://api.moonshot.cn/v1/chat/completions"),
            (
                "moonshot-global",
                O,
                "https://api.moonshot.ai/v1/chat/completions",
            ),
            (
                "baichuan",
                O,
                "https://api.baichuan-ai.com/v1/chat/completions",
            ),
            (
                "dashscope",
                O,
                "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
            ),
            ("stepfun", O, "https://api.stepfun.com/v1/chat/completions"),
            (
                "doubao",
                O,
                "https://ark.cn-beijing.volces.com/api/v3/chat/completions",
            ),
            ("minimax", O, "https://api.minimaxi.com/v1/chat/completions"),
            ("groq", O, "https://api.groq.com/openai/v1/chat/completions"),
            ("together", O, "https://api.together.ai/v1/chat/completions"),
            (
                "fireworks",
                O,
                "https://api.fireworks.ai/inference/v1/chat/completions",
            ),
            (
                "nvidia",
                O,
                "https://integrate.api.nvidia.com/v1/chat/completions",
            ),
            ("grok", O, "https://api.x.ai/v1/chat/completions"),
            ("mistral", O, "https://api.mistral.ai/v1/chat/completions"),
            ("jina", O, "https://api.jina.ai/v1/chat/completions"),
            (
                "perplexity",
                O,
                "https://api.perplexity.ai/chat/completions",
            ),
            (
                "modelscope",
                O,
                "https://api-inference.modelscope.cn/v1/chat/completions",
            ),
            (
                "xirang",
                O,
                "https://wishub-x1.ctyun.cn/v1/chat/completions",
            ),
            (
                "tokenhub",
                O,
                "https://tokenhub.tencentmaas.com/v1/chat/completions",
            ),
            (
                "baidu-cloud",
                O,
                "https://qianfan.baidubce.com/v2/chat/completions",
            ),
            ("gpustack", O, ""),
            (
                "voyageai",
                O,
                "https://api.voyageai.com/v1/chat/completions",
            ),
            ("poe", O, "https://api.poe.com/v1/chat/completions"),
            (
                "longcat",
                O,
                "https://api.longcat.chat/openai/v1/chat/completions",
            ),
            (
                "huggingface",
                O,
                "https://router.huggingface.co/v1/chat/completions",
            ),
            (
                "gateway",
                O,
                "https://ai-gateway.vercel.sh/v1/chat/completions",
            ),
            ("cerebras", O, "https://api.cerebras.ai/v1/chat/completions"),
            ("mimo", O, "https://api.xiaomimimo.com/v1/chat/completions"),
            ("zai", O, "https://api.z.ai/api/paas/v4/chat/completions"),
            (
                "minimax-global",
                O,
                "https://api.minimax.io/v1/chat/completions",
            ),
        ];
        assert_eq!(expected.len(), 56);
        let got = entries();
        let got_ids: Vec<&str> = got.iter().map(|e| e.id.as_str()).collect();
        let want_ids: Vec<&str> = expected.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(got_ids, want_ids);
        for (entry, (id, kind, url)) in got.iter().zip(expected) {
            assert_eq!(&entry.kind, kind, "{id}");
            if url.is_empty() {
                assert_eq!(entry.base_url, "", "{id}: no public address stays empty");
                continue;
            }
            // 预设是填表建议，保存时照常过 normalize：存储值必须原样通过，且落到对照地址。
            let stored = provider_url::normalize(kind, &entry.base_url);
            assert_eq!(
                stored, entry.base_url,
                "{id}: preset must survive normalize"
            );
            assert_eq!(provider_url::request_url(kind, &stored), *url, "{id}");
        }
    }

    #[test]
    fn override_table_wins_over_snapshot_address() {
        let g = entries().into_iter().find(|e| e.id == "gateway").unwrap();
        assert_eq!(g.base_url, "https://ai-gateway.vercel.sh/v1");
    }

    #[test]
    fn perplexity_preset_is_the_full_endpoint() {
        // 无版本段的地址：写主机会被 normalize 补 /v1（404），只能存完整端点（规则①）。
        let p = entries()
            .into_iter()
            .find(|e| e.id == "perplexity")
            .unwrap();
        assert_eq!(p.base_url, "https://api.perplexity.ai/chat/completions");
    }

    #[test]
    fn metadata_comes_from_snapshot_and_extras() {
        let m = meta("deepseek").unwrap();
        assert_eq!(m.icon_key, "deepseek");
        assert!(m.website.starts_with("https://"));
        assert!(!m.sample_models.is_empty() && m.sample_models.len() <= 3);
        // 被排除的条目也给元数据（手工 Gemini 借图标）。
        assert_eq!(meta("gemini").unwrap().icon_key, "google");
        assert!(meta("no-such").is_none());
    }

    fn kind() -> impl Strategy<Value = ProviderKind> {
        prop_oneof![Just(ProviderKind::OpenAi), Just(ProviderKind::Anthropic)]
    }

    fn final_url() -> impl Strategy<Value = (ProviderKind, String)> {
        let host = prop_oneof![
            Just("https://api.example.com"),
            Just("http://localhost:8000"),
            Just("https://x.cn"),
        ];
        let path = proptest::collection::vec(
            prop_oneof![
                Just("v1"),
                Just("v3"),
                Just("api"),
                Just("openai"),
                Just("paas"),
                Just("v1beta")
            ],
            0..4,
        );
        (kind(), host, path).prop_map(|(k, h, p)| {
            let tail = match k {
                ProviderKind::Anthropic => "/messages",
                _ => "/chat/completions",
            };
            let mid: String = p.iter().map(|s| format!("/{s}")).collect();
            (k, format!("{h}{mid}{tail}"))
        })
    }

    proptest! {
        /// 反推的存储值保存后原样、请求恰好打到目标地址（D12：新判定面配属性测试）。
        #[test]
        fn fit_base_round_trips((k, target) in final_url()) {
            let base = fit_base(&k, &target);
            prop_assert_eq!(provider_url::normalize(&k, &base), base.clone());
            prop_assert_eq!(provider_url::request_url(&k, &base), target);
        }
    }
}
