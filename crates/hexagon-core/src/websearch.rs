//! web 搜索摘要槽（code-search-and-subagent 票 03）：可选的搜索供应商槽。
//!
//! 边界：
//! - 只回 {title, url, snippet} 三件套——页面正文仍走既有 `web_fetch`
//!   工具（它带逐跳 Egress 判定），搜索槽不变成「绕过权限的取页面通道」。
//! - 凭据走 `CredentialStore`（keychain/文件回退），key 名 `search/<backend>`；
//!   明文只进 set()，不进项目文件/事件/结果。
//! - 供应商原生搜索在场时本地槽不上工具清单（`Registry::defs_for` 按
//!   `caps` 含 `web` 判定）——同名 `web_search` 会与 Anthropic 服务端
//!   工具撞名，且「二选一」本就是规格。
//! - 自建元搜索（searxng 等）：`backend="custom"` + `endpoint` 预留，
//!   实现按 Brave 同款 JSON 契约解析；本票不验收它。

use crate::tools::ToolError;
use serde_json::Value;
use std::sync::Arc;

/// 单次搜索返回条数上限：摘要是给模型挑「开哪个页面」的，不是数据源。
pub const RESULT_CAP: usize = 10;
/// 查询词长度上限：搜索词来自任务意图，不是把文件正文塞进外发请求
/// （票 04 对子代理的同一约束提前落在工具层——谁调都过不了这条）。
pub const QUERY_CAP: usize = 300;
const SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// 一条搜索摘要（刻意窄——没有正文、没有评分细节）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// 搜索后端接缝（spec：「供应商抽象，测试用替身」）。
pub trait SearchBackend: Send + Sync {
    /// 后端名：进结果元信息与凭据名 `search/<name>`。
    fn name(&self) -> &str;
    fn search(&self, query: &str, count: usize) -> Result<Vec<SearchHit>, ToolError>;
}

/// 当前配置的搜索后端；未配置 → None（工具回报「槽未配置」而非隐式失败）。
/// store 走 Arc——后端实例生命周期不绑调用栈（ctx 持有它跨回合）。
pub fn configured(
    creds: Arc<dyn crate::credentials::CredentialStore>,
) -> Option<Arc<dyn SearchBackend>> {
    let cfg = crate::provider_config::load().ok()?.search?;
    Some(backend_for(&cfg, creds))
}

fn backend_for(
    cfg: &crate::provider_config::SearchCfg,
    creds: Arc<dyn crate::credentials::CredentialStore>,
) -> Arc<dyn SearchBackend> {
    match cfg.backend.as_str() {
        "custom" => Arc::new(GenericJson {
            endpoint: cfg.endpoint.clone().unwrap_or_default(),
            creds,
        }),
        _ => Arc::new(Brave { creds }),
    }
}

/// Brave Web Search API。响应只取 web.results 的 title/url/description——
/// 其余字段（含可能回传的富摘要）一律丢弃，「三件套」是硬输出契约。
struct Brave {
    creds: Arc<dyn crate::credentials::CredentialStore>,
}

impl SearchBackend for Brave {
    fn name(&self) -> &str {
        "brave"
    }
    fn search(&self, query: &str, count: usize) -> Result<Vec<SearchHit>, ToolError> {
        let key = self
            .creds
            .get(&key_name("brave"))
            .map_err(|e| ToolError::Exec(format!("credential store: {e}")))?
            .ok_or_else(|| {
                ToolError::BadInput("search slot configured (brave) but key missing".into())
            })?;
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(SEARCH_TIMEOUT))
            .build()
            .new_agent();
        let resp = agent
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &key)
            .query("q", query)
            .query("count", count.to_string())
            .call()
            .map_err(|e| ToolError::Exec(format!("search request: {e}")))?;
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|e| ToolError::Exec(format!("search response: {e}")))?;
        Ok(parse_brave(&body))
    }
}

fn parse_brave(body: &Value) -> Vec<SearchHit> {
    body["web"]["results"]
        .as_array()
        .map(|rs| {
            rs.iter()
                .filter_map(|r| {
                    let url = r["url"].as_str()?;
                    Some(SearchHit {
                        title: r["title"].as_str().unwrap_or("").to_string(),
                        url: url.to_string(),
                        snippet: r["description"].as_str().unwrap_or("").to_string(),
                    })
                })
                .take(RESULT_CAP)
                .collect()
        })
        .unwrap_or_default()
}

/// 自建元搜索预留（searxng JSON 契约同款字段）。endpoint 为空即未配置。
struct GenericJson {
    endpoint: String,
    creds: Arc<dyn crate::credentials::CredentialStore>,
}

impl SearchBackend for GenericJson {
    fn name(&self) -> &str {
        "custom"
    }
    fn search(&self, query: &str, count: usize) -> Result<Vec<SearchHit>, ToolError> {
        if self.endpoint.is_empty() {
            return Err(ToolError::BadInput(
                "search backend 'custom' needs an endpoint".into(),
            ));
        }
        let mut req = ureq::Agent::config_builder()
            .timeout_global(Some(SEARCH_TIMEOUT))
            .build()
            .new_agent()
            .get(&self.endpoint)
            .query("q", query)
            .query("format", "json");
        if let Some(key) = self
            .creds
            .get(&key_name("custom"))
            .map_err(|e| ToolError::Exec(format!("credential store: {e}")))?
        {
            req = req.header("Authorization", &format!("Bearer {key}"));
        }
        let body: Value = req
            .call()
            .map_err(|e| ToolError::Exec(format!("search request: {e}")))?
            .into_body()
            .read_json()
            .map_err(|e| ToolError::Exec(format!("search response: {e}")))?;
        Ok(body["results"]
            .as_array()
            .map(|rs| {
                rs.iter()
                    .filter_map(|r| {
                        Some(SearchHit {
                            title: r["title"].as_str()?.to_string(),
                            url: r["url"].as_str()?.to_string(),
                            snippet: r["content"].as_str().unwrap_or("").to_string(),
                        })
                    })
                    .take(count.min(RESULT_CAP))
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// 凭据名规约：`search/<backend>`（与 `provider/<id>`/`model/<slot>` 同族）。
pub fn key_name(backend: &str) -> String {
    format!("search/{backend}")
}

// ---------- 工具 ----------

/// `web_search` 工具（票 03）：Egress 类（出网即必问轴）；原生搜索在场
/// 时由 defs_for 从清单摘除——槽在场与否看 `ctx.websearch`。
pub struct WebSearch;

impl crate::tools::Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web via the configured search slot. Returns title+url+snippet only — open a result with web_fetch (each fetch re-runs the egress permission check)."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type":"object","properties":{
            "query":{"type":"string","description":format!("search query (≤{QUERY_CAP} chars)")},
            "count":{"type":"integer","description":"max results (default 5, cap 10)"}},
            "required":["query"]})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::Egress
    }
    fn exec(
        &self,
        _db: &crate::db::Db,
        input: &Value,
        ctx: &crate::tools::ToolContext,
    ) -> Result<Value, ToolError> {
        // 票 03 二选一裁决的执行侧兜底：defs_for_ctx 在 web 槽在场时已
        // 不上清单，模型手写同名调用走到这里照样拒——规格是「用供应商的
        // 搜索，不再调用这个槽」，不是「搜得到就放行」。
        if ctx.caps.contains("web") {
            return Err(ToolError::BadInput(
                "model slot has native web search — local search slot must not be used".into(),
            ));
        }
        let query = crate::tools::str_arg(input, "query")?.trim();
        if query.is_empty() {
            return Err(ToolError::BadInput("empty query".into()));
        }
        let query = query.chars().take(QUERY_CAP).collect::<String>();
        let count = input["count"]
            .as_u64()
            .unwrap_or(5)
            .clamp(1, RESULT_CAP as u64) as usize;
        let Some(backend) = &ctx.websearch else {
            return Err(ToolError::BadInput(
                "web search slot not configured (set providers.json `search`)".into(),
            ));
        };
        let hits = backend.search(&query, count)?;
        Ok(serde_json::json!({
            "backend": backend.name(),
            "count": hits.len(),
            "results": hits.iter().map(|h| serde_json::json!({
                "title": h.title, "url": h.url, "snippet": h.snippet,
            })).collect::<Vec<_>>(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[test]
    fn brave_parse_keeps_three_fields() {
        let body = serde_json::json!({
            "web": {"results": [
                {"title": "T", "url": "https://x", "description": "d", "extra": "drop me"},
                {"title": "T2", "url": "https://y"}
            ]}
        });
        let hits = parse_brave(&body);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "T");
        assert_eq!(hits[0].snippet, "d");
        assert_eq!(hits[1].snippet, "");
    }

    struct Stub(Vec<SearchHit>);
    impl SearchBackend for Stub {
        fn name(&self) -> &str {
            "stub"
        }
        fn search(&self, _q: &str, _c: usize) -> Result<Vec<SearchHit>, ToolError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn tool_returns_snippets_only() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let ctx = crate::tools::ToolContext {
            repo_root: dir.path().to_path_buf(),
            websearch: Some(Arc::new(Stub(vec![SearchHit {
                title: "t".into(),
                url: "https://x".into(),
                snippet: "s".into(),
            }]))),
            ..crate::tools::ToolContext::for_agent(&db, dir.path(), "a")
        };
        let v = WebSearch
            .exec(&db, &serde_json::json!({"query": "q"}), &ctx)
            .unwrap();
        assert_eq!(v["count"], 1);
        assert_eq!(v["results"][0]["url"], "https://x");
        assert!(v["results"][0].get("body").is_none());
    }

    #[test]
    fn unconfigured_slot_reports_not_configured() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let ctx = crate::tools::ToolContext::for_agent(&db, dir.path(), "a");
        let e = WebSearch
            .exec(&db, &serde_json::json!({"query": "q"}), &ctx)
            .unwrap_err();
        assert!(e.to_string().contains("not configured"));
    }
}
