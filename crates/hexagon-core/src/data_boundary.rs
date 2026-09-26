//! Reliability 22 / D03: describe recipients without exposing authentication.
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DataRecipient {
    pub kind: String,
    pub endpoint: Option<String>,
    pub transport: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DataBoundary {
    pub credential_backend: String,
    pub project_open: bool,
    pub recipients: Vec<DataRecipient>,
}

/// Even URL paths can contain bearer tokens. Display only HTTP(S) origins;
/// false negatives hide a destination detail, false positives disclose a secret.
/// D03 therefore drops all userinfo, paths, queries and fragments, including
/// unknown authentication parameter names. Invalid URLs produce no raw fallback.
pub(crate) fn endpoint(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !url.origin().is_tuple() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

pub(crate) fn read(
    root: Option<&Path>,
    store: &dyn crate::credentials::CredentialStore,
    doc: &crate::provider_config::ProviderDoc,
) -> DataBoundary {
    let mut recipients: Vec<_> = doc
        .providers
        .iter()
        .map(|p| DataRecipient {
            kind: "model".into(),
            endpoint: endpoint(&p.base_url),
            transport: "http".into(),
            enabled: p.enabled,
        })
        .collect();
    recipients.extend(crate::mcp::list_mcp_entries(root).into_iter().map(|s| {
        DataRecipient {
            kind: "mcp".into(),
            endpoint: s
                .url
                .as_deref()
                .and_then(|s| endpoint(s.split(" [stored:").next().unwrap_or(s))),
            transport: s.transport,
            enabled: !s.disabled,
        }
    }));
    if let Some(search) = &doc.search {
        recipients.push(DataRecipient {
            kind: "search".into(),
            endpoint: if search.backend == "custom" {
                search.endpoint.as_deref().and_then(endpoint)
            } else {
                Some("https://api.search.brave.com".into())
            },
            transport: "http".into(),
            enabled: true,
        });
    }
    DataBoundary {
        credential_backend: store.backend_kind().into(),
        project_open: root.is_some(),
        recipients,
    }
}

const STORED: &str = "[stored:";
type References = std::collections::HashMap<String, [u8; 32]>;
fn references() -> &'static std::sync::Mutex<References> {
    static REFERENCES: std::sync::OnceLock<std::sync::Mutex<References>> =
        std::sync::OnceLock::new();
    REFERENCES.get_or_init(Default::default)
}
fn fingerprint(raw: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(raw.as_bytes()).into()
}
/// Reliability 22 review: never expose a naked hash of a password/PIN. Each
/// display gets an unrelated OS-random reference; its value fingerprint stays
/// solely in host memory. Restart or bounded-cache expiry requires reloading.
pub(crate) fn hidden(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let mut nonce = [0u8; 24];
    if getrandom::fill(&mut nonce).is_err() {
        return "[stored:unavailable]".into();
    }
    let token = format!(
        "{STORED}{}]",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let mut refs = references().lock().unwrap_or_else(|e| e.into_inner());
    if refs.len() >= 16384 {
        refs.clear();
    }
    refs.insert(token.clone(), fingerprint(raw));
    token
}
pub(crate) fn matches_reference(token: &str, current: &str) -> bool {
    references()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(token)
        == Some(&fingerprint(current))
}
pub(crate) fn hidden_url(raw: &str) -> String {
    format!("{} {}", endpoint(raw).unwrap_or_default(), hidden(raw))
        .trim()
        .into()
}
pub(crate) fn restore(
    input: &str,
    current: Option<&str>,
    is_url: bool,
) -> Result<String, &'static str> {
    if !input.contains(STORED) {
        return Ok(input.into());
    }
    let Some(current) = current else {
        return Err(stale_configuration());
    };
    let marker = input
        .rfind(STORED)
        .map(|i| &input[i..])
        .ok_or_else(stale_configuration)?;
    let expected = if is_url {
        format!("{} {}", endpoint(current).unwrap_or_default(), marker)
            .trim()
            .to_string()
    } else {
        marker.to_string()
    };
    if input != expected || !matches_reference(marker, current) {
        return Err(stale_configuration());
    }
    Ok(current.into())
}

fn stale_configuration() -> &'static str {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        None,
        None,
        None,
        None,
        "configuration_write",
        "stale_binding",
        std::time::Instant::now(),
    );
    "stored configuration changed; reload before saving"
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use sha2::Digest;
    proptest! {
        #[test]
        fn only_origin_survives(secret in "[a-zA-Z0-9]{1,80}", key in "[a-zA-Z]{1,30}") {
            let raw = format!("https://user:{secret}@example.test:8443/{secret}?{key}={secret}#{secret}");
            prop_assert_eq!(endpoint(&raw), Some("https://example.test:8443".into()));
        }
        #[test]
        fn concealed_values_require_the_same_host_value(secret in "[a-zA-Z0-9]{1,80}") {
            let original = format!("old:{secret}");
            let changed = format!("new:{secret}");
            prop_assert!(restore(&hidden(&original), Some(&changed), false).is_err());
            let url = format!("https://example.test/{secret}");
            prop_assert!(restore(&hidden_url(&url), None, true).is_err());
        }
        #[test]
        fn repeated_secrets_have_unrelated_references(secret in "[a-zA-Z0-9]{1,80}") {
            let first = hidden(&secret); let second = hidden(&secret);
            prop_assert_ne!(&first, &second);
            prop_assert!(matches_reference(&first, &secret));
            prop_assert!(matches_reference(&second, &secret));
            let naked_hash = format!("{:x}", sha2::Sha256::digest(secret.as_bytes()));
            prop_assert!(!first.contains(&naked_hash));
        }
        #[test]
        fn unsupported_schemes_never_echo_input(value in ".{0,120}") {
            prop_assert_eq!(endpoint(&format!("file:///{value}")), None);
        }
    }
}
