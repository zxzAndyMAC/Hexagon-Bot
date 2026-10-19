//! 内置服务商预设（gap B8，ui-redesign 2026-10-15；entry#wz-provider 三字段表单
//! 「服务商 ▾ / API Key / 默认模型」）。票 10 的向导服务商步要负责人手填名称、类型、
//! base URL 五个框；原型只问服务商、钥匙、模型三件事——预设把前三项收成一个下拉。
//!
//! 预设只是填表建议，不是第二份供应商存储：选中后界面照常经 `save_provider` 写进
//! providers.json，之后与手填的供应商没有区别。预设表只在 core 一处维护，设置页与
//! 向导读同一份（`provider_presets` 读组命令，静态数据，不碰工作台）。
//!
//! gap B13（2026-10-15）：设置「模型服务」的内置目录原先是 UI 里一份 14 条的硬编码表
//! （ProviderManager PRESETS），与这里的 5 条各说各话（DeepSeek 地址一个带 /v1 一个不带）。
//! 现在两边只读这一份：设置目录列全部；向导只列「能当 default 主对话槽、且要钥匙」的子集——
//! Jev（执行判定专用类型）与本地端点（Ollama / LM Studio，不需要钥匙，而向导放行要求钥匙已存）
//! 由界面过滤掉。否决的替代：DTO 加 `wizard: bool` 标记——过滤条件能从 kind / base_url 推出，
//! 再存一个布尔就是第二份真相。
//!
//! base URL 口径跟 `provider::HttpProvider` 的拼路径规则走：anthropic 类型自带 `/v1/messages`，
//! 所以只写主机；openai 兼容类型拼 `/chat/completions`，所以写到版本前缀为止。
//! 票 21 起拼路径统一在 `provider_url::request_url`，预设值按旧口径的请求地址逐字不变
//! （`provider_url` 测试 `saved_providers_keep_their_request_url` 锁住）。

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
        // 以下来自设置页旧硬编码目录（gap B13 并表，地址原样沿用）。
        // ai.google.dev/gemini-api/docs/openai：OpenAI 兼容端点。
        preset(
            "gemini",
            "Gemini",
            ProviderKind::OpenAi,
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "",
        ),
        preset(
            "xai",
            "xAI",
            ProviderKind::OpenAi,
            "https://api.x.ai/v1",
            "",
        ),
        preset(
            "groq",
            "Groq",
            ProviderKind::OpenAi,
            "https://api.groq.com/openai/v1",
            "",
        ),
        preset(
            "mistral",
            "Mistral",
            ProviderKind::OpenAi,
            "https://api.mistral.ai/v1",
            "",
        ),
        preset(
            "siliconflow",
            "SiliconFlow",
            ProviderKind::OpenAi,
            "https://api.siliconflow.cn/v1",
            "",
        ),
        preset(
            "zhipu",
            "Zhipu",
            ProviderKind::OpenAi,
            "https://open.bigmodel.cn/api/paas/v4",
            "",
        ),
        // 本地端点：不需要钥匙；向导不列（见模块注释）。
        preset(
            "ollama",
            "Ollama",
            ProviderKind::OpenAi,
            "http://localhost:11434/v1",
            "",
        ),
        preset(
            "lm-studio",
            "LM Studio",
            ProviderKind::OpenAi,
            "http://localhost:1234/v1",
            "",
        ),
        // 执行判定（jev 槽）专用；设置页保存时只绑 jev 槽。向导不列（见模块注释）。
        preset(
            "typesafe",
            "TypeSafe",
            ProviderKind::Jev,
            "https://api.typesafe.ai",
            "jev-latest",
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
            // 远程端点一律 https；明文 http 只许本机端点（gap B13 并入 Ollama / LM Studio）。
            assert!(
                p.base_url.starts_with("https://") || p.base_url.starts_with("http://localhost:"),
                "{}",
                p.base_url
            );
            assert!(!p.base_url.ends_with('/'), "{}", p.base_url);
            // Jev 不是聊天模型类型，预设必须自带模型（设置页保存即绑 jev 槽，不能绑空模型）。
            if p.kind == ProviderKind::Jev {
                assert!(!p.default_model.is_empty(), "{}", p.id);
            }
        }
    }

    #[test]
    fn wizard_default_stays_first_and_settings_catalog_is_merged() {
        // gap B13：向导空表时默认落第一条（原型 entry#wz-provider 是 Anthropic）；
        // 设置页旧目录里的服务商都在这一份里，不再有 UI 侧第二份表。
        let all = presets();
        assert_eq!(all[0].id, "anthropic");
        for id in [
            "openai",
            "deepseek",
            "moonshot",
            "openrouter",
            "gemini",
            "xai",
            "groq",
            "mistral",
            "siliconflow",
            "zhipu",
            "ollama",
            "lm-studio",
            "typesafe",
        ] {
            assert!(all.iter().any(|p| p.id == id), "missing preset {id}");
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
