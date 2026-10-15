//! 内置服务商预设（gap B8，ui-redesign 2026-10-15；entry#wz-provider 三字段表单
//! 「服务商 ▾ / API Key / 默认模型」）。票 10 的向导服务商步要负责人手填名称、类型、
//! base URL 五个框；原型只问服务商、钥匙、模型三件事——预设把前三项收成一个下拉。
//!
//! 预设只是填表建议，不是第二份供应商存储：选中后界面照常经 `save_provider` 写进
//! providers.json，之后与手填的供应商没有区别。预设表只在 core 一处维护，设置页与
//! 向导读同一份（`provider_presets` 读组命令，静态数据，不碰工作台）。
//!
//! base URL 口径跟 `provider::HttpProvider` 的拼路径规则走：anthropic 类型自带 `/v1/messages`，
//! 所以只写主机；openai 兼容类型拼 `/chat/completions`，所以写到版本前缀为止。

use crate::provider::ProviderKind;

/// 一个服务商预设（IPC DTO）。`default_model` 为空表示不替负责人猜模型。
#[derive(Debug, Clone, PartialEq, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ProviderPreset {
    /// 建议的供应商 id（slug）；已有同 id 供应商时界面直接选那一个，不另建。
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub default_model: String,
}

fn preset(id: &str, name: &str, kind: ProviderKind, base_url: &str, model: &str) -> ProviderPreset {
    ProviderPreset {
        id: id.into(),
        name: name.into(),
        kind,
        base_url: base_url.into(),
        default_model: model.into(),
    }
}

/// 内置预设表（顺序即下拉顺序）。地址出处：各服务商 API 文档的 base URL 段。
pub fn presets() -> Vec<ProviderPreset> {
    vec![
        // docs.anthropic.com/en/api/messages：POST https://api.anthropic.com/v1/messages。
        // 默认模型取原型 entry#wz-provider 所示。
        preset(
            "anthropic",
            "Anthropic",
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-opus-5-5",
        ),
        // platform.openai.com/docs/api-reference：base https://api.openai.com/v1。
        preset(
            "openai",
            "OpenAI",
            ProviderKind::OpenAi,
            "https://api.openai.com/v1",
            "",
        ),
        // api-docs.deepseek.com：base_url https://api.deepseek.com（OpenAI 兼容）。
        preset(
            "deepseek",
            "DeepSeek",
            ProviderKind::OpenAi,
            "https://api.deepseek.com",
            "",
        ),
        // platform.moonshot.cn/docs：base_url https://api.moonshot.cn/v1（OpenAI 兼容）。
        preset(
            "moonshot",
            "Moonshot",
            ProviderKind::OpenAi,
            "https://api.moonshot.cn/v1",
            "",
        ),
        // openrouter.ai/docs/quickstart：base https://openrouter.ai/api/v1（OpenAI 兼容）。
        preset(
            "openrouter",
            "OpenRouter",
            ProviderKind::OpenAi,
            "https://openrouter.ai/api/v1",
            "",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_unique_slug_ids_and_https_hosts() {
        let all = presets();
        assert!(!all.is_empty());
        let mut ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "preset ids must be unique");
        for p in &all {
            assert!(
                p.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} must be a slug (same rule as the UI providerSlug)",
                p.id
            );
            assert!(!p.name.trim().is_empty());
            assert!(p.base_url.starts_with("https://"), "{}", p.base_url);
            assert!(!p.base_url.ends_with('/'), "{}", p.base_url);
            // 不是聊天模型的类型不能进预设（向导只配 default 主对话槽）。
            assert_ne!(p.kind, ProviderKind::Jev);
        }
    }

    #[test]
    fn anthropic_preset_targets_host_without_version_prefix() {
        // HttpProvider 对 anthropic 类型自拼 /v1/messages；预设再带 /v1 会变成 /v1/v1/messages。
        let a = presets().into_iter().find(|p| p.id == "anthropic").unwrap();
        assert_eq!(a.kind, ProviderKind::Anthropic);
        assert!(!a.base_url.ends_with("/v1"));
    }
}
