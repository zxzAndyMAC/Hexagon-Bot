//! 向导服务商步的实时密钥检测（gap B4，ui-redesign 2026-10-15；entry#wz-provider
//! 「检测通过 · 200K 窗口」）。票 10 时没有检测命令，负责人只能保存后到建项目最后一步
//! 才知道钥匙对不对。
//!
//! 检测 = 用候选钥匙请求一次该服务商的模型目录（与设置页「拉取模型」同一请求，
//! `provider_config::fetch_catalog`），不发对话请求、不花 token、不落盘。
//! 失败必须带原因词（`CatalogFailure::code`），界面按词给可操作的说明。
//!
//! 代价模型：检测是只读的建议，不是放行闸——放行仍看 default 槽是否启用且已存钥匙
//! （`providerStepReady`）。检测误报失败（false negative）花负责人一次「仍然保存」，
//! 误报通过（false positive）只会在第一次真实请求时以模型恢复事项暴露；两边都不产生副作用，
//! 所以实现不偏向任何一边，原样报告服务商的回应。

use crate::credentials::{provider_key_name, CredentialStore};
use crate::provider_config::{self, ProviderDef};

/// 一次检测的结果（IPC DTO）。`ok=false` 时 `failure` 必有。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct KeyProbe {
    /// 钥匙与地址可用：目录请求成功。
    pub ok: bool,
    /// 失败原因词：authentication / quota / rate_limited / provider_unavailable /
    /// request_rejected / endpoint_not_found / transport / deadline / invalid_response /
    /// invalid_url / missing_credential。
    pub failure: Option<String>,
    /// 服务商回的 HTTP 状态（有才给）。
    pub status: Option<u16>,
    /// 所填模型是否在服务商目录里；没填模型或目录为空时不判（None）。
    pub model_listed: Option<bool>,
    /// 所填模型的上下文窗口（目录元数据或内置前缀表；认不出为 None）。
    #[ts(type = "number | null")]
    pub window: Option<u64>,
}

impl KeyProbe {
    fn failed(code: &str, status: Option<u16>) -> Self {
        Self {
            ok: false,
            failure: Some(code.to_string()),
            status,
            model_listed: None,
            window: None,
        }
    }
}

/// 用候选钥匙检测 `def`。`secret` 为空时用钥匙串里已存的那把（已存钥匙的供应商不再要求重输）。
/// 地址经与保存同一道 `data_boundary::restore`：界面拿到的是遮蔽过的地址，原样送回时还原。
pub fn probe_key(
    def: ProviderDef,
    secret: Option<String>,
    model: &str,
    store: &dyn CredentialStore,
) -> Result<KeyProbe, crate::provider_admin::AdminError> {
    let doc = provider_config::load()?;
    let current = doc.providers.iter().find(|p| p.id == def.id);
    let Ok(base_url) =
        crate::data_boundary::restore(&def.base_url, current.map(|p| p.base_url.as_str()), true)
    else {
        return Ok(KeyProbe::failed("invalid_url", None));
    };
    if base_url.trim().is_empty() {
        return Ok(KeyProbe::failed("invalid_url", None));
    }
    let typed = secret
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let key = match typed {
        Some(k) => Some(k),
        None => store
            .get(&provider_key_name(&def.id))?
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty()),
    };
    let Some(key) = key else {
        return Ok(KeyProbe::failed("missing_credential", None));
    };
    let def = ProviderDef { base_url, ..def };
    let catalog = match provider_config::fetch_catalog(&def, &key) {
        Ok(c) => c,
        Err(f) => return Ok(KeyProbe::failed(f.code, f.status)),
    };
    let model = model.trim();
    let model_listed =
        (!model.is_empty() && !catalog.is_empty()).then(|| catalog.iter().any(|m| m.id == model));
    let window = if model.is_empty() {
        None
    } else {
        provider_config::window_of(
            &ProviderDef {
                models: catalog,
                ..def
            },
            model,
        )
    };
    Ok(KeyProbe {
        ok: true,
        failure: None,
        status: None,
        model_listed,
        window,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;
    use crate::provider::ProviderKind;
    use std::io::{Read, Write};

    /// 一次性本地服务：回 `status` 与 `body`，返回地址与请求头抓取。
    fn serve_once(status: &'static str, body: String) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            let n = socket.read(&mut bytes).unwrap();
            write!(
                socket,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            String::from_utf8_lossy(&bytes[..n]).into_owned()
        });
        (format!("http://{address}"), server)
    }

    fn def(base_url: String) -> ProviderDef {
        ProviderDef {
            id: "probe".into(),
            name: "Probe".into(),
            kind: ProviderKind::OpenAi,
            base_url,
            models: vec![],
            enabled: true,
        }
    }

    #[test]
    fn listed_model_passes_with_window_and_typed_key_is_sent() {
        let _doc = provider_config::fixture_document(Default::default());
        let body = serde_json::json!({"data":[{"id":"m-1","context_length":200000}]}).to_string();
        let (url, server) = serve_once("200 OK", body);
        let got = probe_key(
            def(url),
            Some(" sk-typed ".into()),
            "m-1",
            &MemoryStore::default(),
        )
        .unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /models"));
        assert!(request.contains("Bearer sk-typed"));
        assert_eq!(
            got,
            KeyProbe {
                ok: true,
                failure: None,
                status: None,
                model_listed: Some(true),
                window: Some(200000),
            }
        );
    }

    #[test]
    fn unlisted_model_is_reported_but_key_still_passes() {
        let _doc = provider_config::fixture_document(Default::default());
        let body = serde_json::json!({"data":[{"id":"other"}]}).to_string();
        let (url, server) = serve_once("200 OK", body);
        let got = probe_key(def(url), Some("k".into()), "m-1", &MemoryStore::default()).unwrap();
        server.join().unwrap();
        assert!(got.ok);
        assert_eq!(got.model_listed, Some(false));
    }

    #[test]
    fn stored_key_is_used_when_none_is_typed() {
        use crate::credentials::CredentialStore;
        let _doc = provider_config::fixture_document(Default::default());
        let store = MemoryStore::default();
        store.set("provider/probe", "sk-stored").unwrap();
        let (url, server) = serve_once("200 OK", r#"{"data":[]}"#.into());
        let got = probe_key(def(url), None, "", &store).unwrap();
        assert!(server.join().unwrap().contains("Bearer sk-stored"));
        assert!(got.ok);
        assert_eq!(got.model_listed, None, "没填模型不判");
    }

    #[test]
    fn failures_carry_a_reason() {
        let _doc = provider_config::fixture_document(Default::default());
        for (status, code) in [
            ("401 Unauthorized", "authentication"),
            ("404 Not Found", "endpoint_not_found"),
            ("429 Too Many Requests", "rate_limited"),
            ("503 Service Unavailable", "provider_unavailable"),
        ] {
            let (url, server) = serve_once(status, "{}".into());
            let got = probe_key(def(url), Some("k".into()), "m", &MemoryStore::default()).unwrap();
            server.join().unwrap();
            assert!(!got.ok);
            assert_eq!(got.failure.as_deref(), Some(code), "{status}");
            assert!(got.status.is_some());
        }
        let (url, server) = serve_once("200 OK", r#"{"nope":1}"#.into());
        let got = probe_key(def(url), Some("k".into()), "m", &MemoryStore::default()).unwrap();
        server.join().unwrap();
        assert_eq!(got.failure.as_deref(), Some("invalid_response"));

        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let got = probe_key(def(url), Some("k".into()), "m", &MemoryStore::default()).unwrap();
        assert_eq!(got.failure.as_deref(), Some("transport"));

        let got = probe_key(
            def("not a url".into()),
            Some("k".into()),
            "m",
            &MemoryStore::default(),
        )
        .unwrap();
        assert_eq!(got.failure.as_deref(), Some("invalid_url"));
        let got = probe_key(
            def("http://127.0.0.1:9".into()),
            None,
            "m",
            &MemoryStore::default(),
        )
        .unwrap();
        assert_eq!(got.failure.as_deref(), Some("missing_credential"));
    }

    /// owner-followups-1015 #03：设置页「检测并拉取」曾先 probe 再 fetch，同一目录请求两次；
    /// 改为只调 `provider_admin::fetch_models` 一次后，失败码必须与连接检测同一套词，
    /// 界面才能给同样可操作的原因（而不是笼统的「provider request or response failed」）。
    #[test]
    fn fetch_models_failures_carry_the_probe_reason() {
        use crate::errcode::ErrorCode;
        let dir = tempfile::tempdir().unwrap();
        let _path = provider_config::fixture_path(dir.path().join("providers.json"));
        let store = MemoryStore::default();
        let fetch_code = |url: String, key: Option<&str>| {
            crate::provider_admin::save(def(url), key.map(Into::into), &store).unwrap();
            crate::provider_admin::fetch_models("probe", &store)
                .unwrap_err()
                .code()
        };

        assert_eq!(
            fetch_code("http://127.0.0.1:9".into(), None),
            "missing_credential"
        );

        let (url, server) = serve_once("401 Unauthorized", "{}".into());
        let code = fetch_code(url, Some("k"));
        server.join().unwrap();
        assert_eq!(code, "authentication");

        let (url, server) = serve_once("200 OK", r#"{"nope":1}"#.into());
        let code = fetch_code(url, Some("k"));
        server.join().unwrap();
        assert_eq!(code, "invalid_response");

        let (url, server) = serve_once("200 OK", r#"{"data":[{"id":"m-1"}]}"#.into());
        crate::provider_admin::save(def(url), None, &store).unwrap();
        let models = crate::provider_admin::fetch_models("probe", &store).unwrap();
        server.join().unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["m-1"]
        );
    }

    proptest::proptest! {
        /// 状态分类器（D12）：每个状态都落到界面认得的词；钥匙类与服务端类不串档。
        #[test]
        fn status_class_is_total_and_stable(status in 100u16..=599) {
            let code = provider_config::status_class(status);
            let known = ["authentication", "quota", "endpoint_not_found", "rate_limited",
                "provider_unavailable", "request_rejected", "invalid_response"];
            proptest::prop_assert!(known.contains(&code));
            proptest::prop_assert_eq!(code == "authentication", status == 401 || status == 403);
            proptest::prop_assert_eq!(code == "provider_unavailable", (500..=599).contains(&status));
            if (400..=499).contains(&status) {
                proptest::prop_assert!(code != "invalid_response");
            }
        }
    }
}
