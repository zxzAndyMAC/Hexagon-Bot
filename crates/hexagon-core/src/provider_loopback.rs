//! 本机端点免钥匙判定（owner-followups-1015 #25；2026-10-18 负责人裁决，采纳编排方建议；
//! 票 03 遗留项）。
//!
//! 出处：Ollama / LM Studio 这类本机服务不要钥匙，但连接检测、「检测并拉取模型」和对话请求
//! 都先查钥匙串，没有就报 `missing_credential`——负责人只能随手填一把假钥匙才能用。
//! 裁决：服务商地址（规范化后的存储值，见 `provider_url`）的主机是回环地址
//! （`localhost`、`127.0.0.0/8`、`[::1]`）且没有钥匙时，不带鉴权头直接请求；
//! 其他主机仍必须有钥匙（fail-closed）。有钥匙时一律照常带头，回环也不例外。
//!
//! 判定代价：误放行（把远端当回环）= 向远端发一次无钥匙请求，只泄露「有人在请求」的存在性，
//! 不泄露钥匙（本来就没有）；误拒（把回环当远端）= 本机用户多填一个假钥匙。两边都轻，
//! 但误放行的对象是不可信的远端，所以实现偏向拒绝：只认解析后的主机，字面量前缀
//! （`localhost.evil.com`、`127.0.0.1.nip.io`）、带用户信息的地址（`http://127.0.0.1@evil.com`
//! 主机是 evil.com；`http://u@localhost` 一并拒绝，不猜意图）、非 http(s) 方案、
//! IPv4 映射的 IPv6（`[::ffff:127.0.0.1]`）、带尾点的 `localhost.` 一律不算。
//! 否决的替代：界面原先的前缀正则 `^(https?://)?(localhost|127.0.0.1|[::1])`——
//! `localhost.evil.com` 也匹配，只能当提示，不能当放行闸。

use std::time::Instant;

/// 判定分支名（诊断记录 `branch`）。
const BRANCH: &str = "keyless_endpoint";

/// `base_url` 解析后的主机是不是回环地址（且没有用户信息、方案是 http/https）。
pub fn is_loopback(base_url: &str) -> bool {
    let Ok(url) = url::Url::parse(base_url.trim()) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// 一次请求用不用钥匙。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// 有钥匙：照常带鉴权头（回环也带）。
    Key(String),
    /// 回环且无钥匙：不带鉴权头。
    Keyless,
    /// 非回环且无钥匙：调用方报 `missing_credential`，不发请求。
    Missing,
}

impl Auth {
    /// 有钥匙时的钥匙；`Keyless` 为 None。`Missing` 不该走到请求，调用方先分流。
    pub fn key(&self) -> Option<&str> {
        match self {
            Auth::Key(k) => Some(k),
            Auth::Keyless | Auth::Missing => None,
        }
    }
}

/// 有钥匙才加鉴权头（`value` 为 None = 回环免钥匙请求，不带头）。
pub(crate) fn header<B>(
    req: ureq::RequestBuilder<B>,
    name: &str,
    value: Option<&str>,
) -> ureq::RequestBuilder<B> {
    match value {
        Some(v) => req.header(name, v),
        None => req,
    }
}

/// 钥匙（已 trim、空串视为没有）+ 地址 → 本次请求的鉴权方式。
/// 只在没有钥匙时才是判定分支，记一条 Debug 诊断（不含地址与钥匙）。
pub fn decide(key: Option<String>, base_url: &str) -> Auth {
    let key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    if let Some(k) = key {
        return Auth::Key(k);
    }
    let t0 = Instant::now();
    let loopback = is_loopback(base_url);
    // 记「宿主」而非「判定」：检测、拉取、对话请求都没有项目上下文可挂（服务商是宿主级配置），
    // 无项目视图只读宿主记录（diag::records），记成「判定」写了也读不出来。
    crate::diag::host(
        BRANCH,
        if loopback {
            "loopback_keyless"
        } else {
            "remote_requires_key"
        },
        t0,
    );
    if loopback {
        Auth::Keyless
    } else {
        Auth::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn loopback_hosts_are_keyless() {
        for url in [
            "http://localhost:11434/v1",
            "http://LOCALHOST:1234",
            "https://localhost/v1",
            "http://127.0.0.1:1234/v1",
            "http://127.1.2.3:8080",
            "http://[::1]:11434/v1",
        ] {
            assert!(is_loopback(url), "{url}");
        }
    }

    #[test]
    fn lookalikes_and_userinfo_are_not_loopback() {
        for url in [
            "http://localhost.evil.com/v1",
            "http://127.0.0.1.nip.io/v1",
            "http://evil.com/localhost",
            "http://127.0.0.1@evil.com/v1",
            "http://user@localhost:11434",
            "http://user:pw@127.0.0.1:11434",
            "http://localhost.:11434",
            "http://[::ffff:127.0.0.1]:11434",
            "localhost:11434",
            "ftp://localhost/v1",
            "file:///tmp/x",
            "https://api.openai.com/v1",
            "",
            "not a url",
        ] {
            assert!(!is_loopback(url), "{url}");
        }
    }

    #[test]
    fn key_wins_and_missing_key_splits_by_host() {
        assert_eq!(
            decide(Some(" k ".into()), "http://localhost"),
            Auth::Key("k".into())
        );
        assert_eq!(decide(Some("  ".into()), "http://localhost"), Auth::Keyless);
        assert_eq!(decide(None, "http://[::1]:1"), Auth::Keyless);
        assert_eq!(decide(None, "https://api.x.com/v1"), Auth::Missing);
    }

    #[test]
    fn keyless_branch_leaves_a_debug_host_record() {
        let _g = crate::diag::TEST_LEVEL_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let before = log::max_level();
        log::set_max_level(log::LevelFilter::Debug);
        decide(None, "http://localhost:1");
        decide(None, "https://remote.example/v1");
        let codes: Vec<String> = crate::diag::records(None, Some(crate::diag::CLASS_HOST))
            .into_iter()
            .filter(|r| r.branch == BRANCH)
            .map(|r| format!("{}:{}", r.level, r.code))
            .collect();
        log::set_max_level(before);
        assert!(
            codes.contains(&"debug:loopback_keyless".to_string()),
            "{codes:?}"
        );
        assert!(
            codes.contains(&"debug:remote_requires_key".to_string()),
            "{codes:?}"
        );
        assert!(codes.iter().all(|c| c.starts_with("debug:")), "{codes:?}");
    }

    fn label() -> impl Strategy<Value = String> {
        "[a-z0-9]{1,8}"
    }

    proptest! {
        /// D12：允许无钥匙 ⇔ 解析后的主机是回环。以 `localhost` / `127.0.0.1` 打头的
        /// 子域名、带用户信息的地址一律不算。
        #[test]
        fn keyless_iff_parsed_host_is_loopback(
            scheme in prop_oneof![Just("http"), Just("https")],
            port in proptest::option::of(1u16..),
            path in "(/[a-z0-9]{0,6}){0,3}",
            sub in label(),
            user in label(),
        ) {
            let port = port.map(|p| format!(":{p}")).unwrap_or_default();
            for host in ["localhost", "127.0.0.1", "[::1]"] {
                let url = format!("{scheme}://{host}{port}{path}");
                prop_assert!(is_loopback(&url), "{}", url);
                prop_assert_eq!(decide(None, &url), Auth::Keyless);
                let lookalike = format!("{scheme}://{host}.{sub}.com{port}{path}");
                if host != "[::1]" {
                    prop_assert!(!is_loopback(&lookalike), "{}", lookalike);
                }
                let with_user = format!("{scheme}://{user}@{host}{port}{path}");
                prop_assert!(!is_loopback(&with_user), "{}", with_user);
                let smuggled = format!("{scheme}://{host}@{sub}.com{port}{path}");
                prop_assert!(!is_loopback(&smuggled), "{}", smuggled);
            }
        }

        /// 任意输入不 panic；放行的必然解析成功且无用户信息（fail-closed 方向）。
        #[test]
        fn arbitrary_input_never_widens(raw in "\\PC{0,40}") {
            if is_loopback(&raw) {
                let u = url::Url::parse(raw.trim()).unwrap();
                prop_assert!(u.username().is_empty() && u.password().is_none());
                prop_assert!(matches!(u.scheme(), "http" | "https"));
            }
        }

        /// 有钥匙永远带钥匙，与主机无关。
        #[test]
        fn a_key_is_always_sent(key in "[a-zA-Z0-9]{1,16}", host in prop_oneof![
            Just("localhost".to_string()), Just("127.0.0.1".to_string()), label().prop_map(|l| format!("{l}.com"))
        ]) {
            prop_assert_eq!(decide(Some(key.clone()), &format!("http://{host}")), Auth::Key(key));
        }
    }
}
