//! 服务商地址的规范化与「实际请求地址」（owner-followups-1015 第 12 项，2026-10-18 负责人裁决；票 21）。
//!
//! 出处：负责人填 base URL 时必须知道 Hexagon「写到版本前缀为止」的私有规则——填主机名
//! `https://api.deepseek.com` 会请求 `/chat/completions`（少 `/v1`），填完整端点会请求
//! `…/chat/completions/chat/completions`，Anthropic 填 `…/v1` 会请求 `/v1/v1/messages`。
//! 裁决的三条规则（只用于用户填写 / 编辑的地址）：
//! ① 以 `/chat/completions` 或 `/messages` 结尾 → 原样；
//! ② 路径含版本段（`/v\d+…`，含 `/api/v3`、`/api/paas/v4`、`/v1beta/openai`）→ 只补端点尾；
//! ③ 否则 OpenAI 兼容补 `/v1/chat/completions`、Anthropic 补 `/v1/messages`。
//!
//! 两层分工，为的是「已保存的服务商行为不变」：
//! - [`normalize`] 只在保存 / 检测负责人新填的地址时跑（`provider_admin::save`、`provider_probe`；
//!   界面拿到的已存地址是遮蔽形 `origin [stored:…]`，原样送回时走 `data_boundary::restore`
//!   还原，不经这里）。OpenAI 兼容的③在这一步把 `/v1` 写进存储值。
//! - [`request_url`] 是运行时唯一的拼路径处（`HttpProvider` 与模型目录都读它）。OpenAI 兼容
//!   对存储值只补 `/chat/completions`（与旧口径逐字相同，所以旧存的 `https://api.deepseek.com`
//!   仍请求 `/chat/completions`）；Anthropic 按①②③拼——旧存的「只写主机」落③，与旧口径相同。
//!
//! 否决的替代：运行时对 OpenAI 兼容也套③——旧存的无版本地址（DeepSeek 预设、Perplexity 自填）
//! 升级后会改请求 `/v1/chat/completions`，Perplexity 因此 404，违反「已保存的服务商行为不变」；
//! 存「最终请求地址」——要迁移 providers.json，模型目录还得从端点反推根，改面大且无收益。
//!
//! 判定代价：误判「含版本段」（把 `/v2ray` 当版本）= 少补一个 `/v1`，表单下方的实际请求地址
//! 当场可见、改一次即可；误判「不含」= 多补 `/v1`，同样当场可见。两边代价对称，实现只看
//! 完整路径段，不做猜测。

use crate::provider::ProviderKind;

const OPENAI_TAIL: &str = "/chat/completions";
const ANTHROPIC_TAIL: &str = "/messages";

/// 去掉 scheme 与主机，取路径（不含查询与片段）。没有 scheme 时整串当路径之外的主机看待。
fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    rest.find('/').map_or("", |i| &rest[i..])
}

/// 路径里有没有完整的版本段：`v` + 数字 + 可选字母数字（`v1`、`v4`、`v1beta`）。
fn has_version_segment(url: &str) -> bool {
    path_of(url).split('/').any(|seg| {
        let mut chars = seg.chars();
        matches!(chars.next(), Some('v' | 'V'))
            && seg[1..].starts_with(|c: char| c.is_ascii_digit())
            && seg[1..].chars().all(|c| c.is_ascii_alphanumeric())
    })
}

fn trimmed(input: &str) -> &str {
    input.trim().trim_end_matches('/')
}

/// 把负责人新填的地址转成存储值。幂等；完整端点原样保留。见模块注释。
pub fn normalize(kind: &ProviderKind, input: &str) -> String {
    let base = trimmed(input);
    match kind {
        ProviderKind::OpenAi
            if !base.is_empty() && !base.ends_with(OPENAI_TAIL) && !has_version_segment(base) =>
        {
            format!("{base}/v1")
        }
        _ => base.to_string(),
    }
}

/// 存储值 → 实际请求地址（对话请求打的那一个 URL）。
pub fn request_url(kind: &ProviderKind, stored: &str) -> String {
    let base = trimmed(stored);
    match kind {
        ProviderKind::OpenAi if base.ends_with(OPENAI_TAIL) => base.to_string(),
        ProviderKind::OpenAi => format!("{base}{OPENAI_TAIL}"),
        ProviderKind::Anthropic if base.ends_with(ANTHROPIC_TAIL) => base.to_string(),
        ProviderKind::Anthropic if has_version_segment(base) => format!("{base}{ANTHROPIC_TAIL}"),
        ProviderKind::Anthropic => format!("{base}/v1{ANTHROPIC_TAIL}"),
        ProviderKind::Jev => crate::provider::systemone_url(base),
    }
}

/// 存储值 → 模型目录地址（`GET …/models`），与 [`request_url`] 同根。
pub fn models_url(kind: &ProviderKind, stored: &str) -> String {
    let base = trimmed(stored);
    match kind {
        ProviderKind::Anthropic => {
            let root = base.strip_suffix(ANTHROPIC_TAIL).unwrap_or(base);
            if has_version_segment(root) {
                format!("{root}/models")
            } else {
                format!("{root}/v1/models")
            }
        }
        _ => {
            let root = base.strip_suffix(OPENAI_TAIL).unwrap_or(base);
            format!("{root}/models")
        }
    }
}

/// 保存 / 检测收到的地址 → 存储值。遮蔽形 = 已存值未改动，经 `data_boundary::restore`
/// 原样还原（不规范化，保证已保存服务商行为不变）；其余是负责人新填的，走 [`normalize`]。
pub(crate) fn resolve_input(
    kind: &ProviderKind,
    input: &str,
    current: Option<&str>,
) -> Result<String, &'static str> {
    if crate::data_boundary::is_hidden(input) {
        crate::data_boundary::restore(input, current, true)
    } else {
        Ok(normalize(kind, input))
    }
}

/// 表单实时预览：负责人正在填的地址最终会请求哪里。遮蔽形（已存地址未改动）与空串
/// 不预览——遮蔽形的路径本就不给界面看（Reliability 22：路径里可能有令牌）。
pub fn preview(kind: &ProviderKind, input: &str) -> Option<String> {
    let base = trimmed(input);
    if base.is_empty() || crate::data_boundary::is_hidden(base) {
        return None;
    }
    Some(request_url(kind, &normalize(kind, base)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn final_url(kind: ProviderKind, typed: &str) -> String {
        request_url(&kind, &normalize(&kind, typed))
    }

    #[test]
    fn deepseek_three_spellings_reach_one_endpoint() {
        for typed in [
            "https://api.deepseek.com",
            "https://api.deepseek.com/",
            "https://api.deepseek.com/v1",
            "https://api.deepseek.com/v1/chat/completions",
        ] {
            assert_eq!(
                final_url(ProviderKind::OpenAi, typed),
                "https://api.deepseek.com/v1/chat/completions",
                "{typed}"
            );
        }
    }

    #[test]
    fn versioned_paths_only_get_the_endpoint_tail() {
        let cases = [
            // 智谱
            (
                "https://open.bigmodel.cn/api/paas/v4",
                "https://open.bigmodel.cn/api/paas/v4/chat/completions",
            ),
            // 豆包（火山方舟）
            (
                "https://ark.cn-beijing.volces.com/api/v3",
                "https://ark.cn-beijing.volces.com/api/v3/chat/completions",
            ),
            // Gemini OpenAI 兼容：版本段不在末尾
            (
                "https://generativelanguage.googleapis.com/v1beta/openai",
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
            ),
        ];
        for (typed, want) in cases {
            assert_eq!(final_url(ProviderKind::OpenAi, typed), want);
        }
    }

    #[test]
    fn perplexity_full_endpoint_is_used_as_is() {
        let typed = "https://api.perplexity.ai/chat/completions";
        assert_eq!(final_url(ProviderKind::OpenAi, typed), typed);
        assert_eq!(
            models_url(
                &ProviderKind::OpenAi,
                &normalize(&ProviderKind::OpenAi, typed)
            ),
            "https://api.perplexity.ai/models"
        );
    }

    #[test]
    fn anthropic_spellings() {
        for typed in [
            "https://api.anthropic.com",
            "https://api.anthropic.com/v1",
            "https://api.anthropic.com/v1/messages",
        ] {
            assert_eq!(
                final_url(ProviderKind::Anthropic, typed),
                "https://api.anthropic.com/v1/messages",
                "{typed}"
            );
            assert_eq!(
                models_url(
                    &ProviderKind::Anthropic,
                    &normalize(&ProviderKind::Anthropic, typed)
                ),
                "https://api.anthropic.com/v1/models"
            );
        }
        assert_eq!(
            final_url(
                ProviderKind::Anthropic,
                "https://api.deepseek.com/anthropic"
            ),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
    }

    /// 已保存的服务商（升级前写进 providers.json 的值）请求地址逐字不变：旧口径
    /// OpenAI 兼容 = `{base}/chat/completions`，Anthropic = `{base}/v1/messages`（只写主机）。
    #[test]
    fn saved_providers_keep_their_request_url() {
        let saved = [
            (
                ProviderKind::OpenAi,
                "https://api.deepseek.com",
                "https://api.deepseek.com/chat/completions",
            ),
            (
                ProviderKind::OpenAi,
                "https://api.perplexity.ai",
                "https://api.perplexity.ai/chat/completions",
            ),
            (
                ProviderKind::OpenAi,
                "https://api.openai.com/v1",
                "https://api.openai.com/v1/chat/completions",
            ),
            (
                ProviderKind::OpenAi,
                "https://openrouter.ai/api/v1/",
                "https://openrouter.ai/api/v1/chat/completions",
            ),
            (
                ProviderKind::OpenAi,
                "http://localhost:11434/v1",
                "http://localhost:11434/v1/chat/completions",
            ),
            (
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                "https://api.anthropic.com/v1/messages",
            ),
            (
                ProviderKind::Jev,
                "https://api.typesafe.ai",
                "https://api.typesafe.ai/v1/systemone",
            ),
        ];
        for (kind, stored, want) in saved {
            assert_eq!(request_url(&kind, stored), want, "{stored}");
        }
        // 只锁手工 14 条（升级前就有、可能已被存下的值）。票 22 的目录条目是新的，其存储值
        // 可能是完整端点（Perplexity），由 provider_catalog 的 56 家对照表锁最终请求地址。
        for p in crate::provider_presets::presets()
            .into_iter()
            .filter(|p| p.origin == crate::provider_presets::PresetOrigin::Manual)
        {
            let old = match p.kind {
                ProviderKind::OpenAi => format!("{}/chat/completions", p.base_url),
                ProviderKind::Anthropic => format!("{}/v1/messages", p.base_url),
                ProviderKind::Jev => crate::provider::systemone_url(&p.base_url),
            };
            assert_eq!(request_url(&p.kind, &p.base_url), old, "{}", p.id);
        }
    }

    #[test]
    fn preview_skips_hidden_and_empty_inputs() {
        assert_eq!(preview(&ProviderKind::OpenAi, "  "), None);
        let hidden = crate::data_boundary::hidden_url("https://api.deepseek.com/secret-token");
        assert_eq!(preview(&ProviderKind::OpenAi, &hidden), None);
        assert_eq!(
            preview(&ProviderKind::OpenAi, "https://api.deepseek.com").as_deref(),
            Some("https://api.deepseek.com/v1/chat/completions")
        );
    }

    fn kind() -> impl Strategy<Value = ProviderKind> {
        prop_oneof![
            Just(ProviderKind::OpenAi),
            Just(ProviderKind::Anthropic),
            Just(ProviderKind::Jev)
        ]
    }

    /// 主机 + 0..3 段路径（段里混入版本形、端点形与普通词），可带尾斜杠。
    fn typed_url() -> impl Strategy<Value = String> {
        let seg = prop_oneof![
            "[a-z]{1,8}",
            "v[0-9]{1,2}",
            "v[0-9]beta",
            Just("api".to_string()),
            Just("chat".to_string()),
            Just("completions".to_string()),
            Just("messages".to_string()),
        ];
        (
            prop_oneof![Just("https"), Just("http")],
            "[a-z]{1,10}\\.[a-z]{2,4}",
            prop::collection::vec(seg, 0..4),
            any::<bool>(),
        )
            .prop_map(|(scheme, host, segs, slash)| {
                let mut s = format!("{scheme}://{host}");
                for seg in segs {
                    s.push('/');
                    s.push_str(&seg);
                }
                if slash {
                    s.push('/');
                }
                s
            })
    }

    proptest! {
        /// 规范化两次等于一次。
        #[test]
        fn normalize_is_idempotent(k in kind(), u in typed_url()) {
            let once = normalize(&k, &u);
            prop_assert_eq!(normalize(&k, &once), once.clone());
            prop_assert_eq!(request_url(&k, &normalize(&k, &once)), request_url(&k, &once));
        }

        /// 填的就是完整端点 → 原样请求。
        #[test]
        fn full_endpoint_is_kept_verbatim(u in typed_url()) {
            let base = u.trim_end_matches('/');
            let openai = format!("{base}/chat/completions");
            prop_assert_eq!(final_url(ProviderKind::OpenAi, &openai), openai);
            let anthropic = format!("{base}/messages");
            prop_assert_eq!(final_url(ProviderKind::Anthropic, &anthropic), anthropic);
        }

        /// 结果总以本类型的端点尾结束，且只补一次版本段（不会出现新的 `/v1/v1`）。
        #[test]
        fn request_ends_with_one_endpoint(u in typed_url()) {
            for k in [ProviderKind::OpenAi, ProviderKind::Anthropic] {
                let got = final_url(k.clone(), &u);
                let tail = if k == ProviderKind::OpenAi { OPENAI_TAIL } else { ANTHROPIC_TAIL };
                prop_assert!(got.ends_with(tail), "{got}");
                if !u.trim_end_matches('/').ends_with(tail) {
                    prop_assert!(has_version_segment(&got), "{got}");
                }
                if !u.contains("/v1/v1") {
                    prop_assert!(!got.contains("/v1/v1"), "{got}");
                }
            }
        }
    }
}
