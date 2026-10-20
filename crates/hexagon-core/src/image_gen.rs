//! 出图执行器（owner-followups-1015 票 20，规格「画师」，2026-10-17 负责人裁决）。
//!
//! 首版只支持 OpenAI 兼容 `POST {root}/images/generations`；读出图槽的**精确**绑定
//! （[`provider_config::image_generation_slot`] 判能力，这里只解析供应商与钥匙），不回退
//! default——default 是对话模型，回退 = 拿对话模型凑合出图（票 19 同一条代价模型）。
//!
//! 外发边界（规格用户故事 73）：请求体只有 `model` / `prompt` / `n` / `size`；`prompt` 只由
//! [`avatar_prompt`] 用角色名、职责摘要、风格描述拼成，不含项目文件与对话内容。调用方在发出
//! **之前**把这三样写进轨迹（`artist_image_request`）。被否决：把画师整段对话上下文交给出图
//! 模型「自由发挥」——外发面不可审计。
//!
//! 钥匙只在 [`ImageAccess`] 里取、只进 Authorization 头；结果图若是 URL，下载时**不带**钥匙
//! （URL 可能指向另一台主机）。失败只带分类码与 HTTP 状态，不带响应正文（Reliability 22：
//! 原文可能含凭据）。

use base64::Engine;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::credentials::CredentialStore;
use crate::provider::ProviderKind;
use crate::provider_config::{self, ProviderDoc};

/// 生成图落地边长：头像只在 18–96px 显示，256 足够且远低于头像 2MB 上限。
pub const AVATAR_SIDE: u32 = 256;
/// 结果图（b64 解码后或 URL 下载）的原始字节上限。超出按 invalid_response 拒。
const MAX_RAW_BYTES: u64 = 16 * 1024 * 1024;
const REQUEST_SIZE: &str = "1024x1024";
/// 职责摘要的最长字符数：只取第一句，免得把整段职责（含协作线索）外发。
const DUTY_SUMMARY_CHARS: usize = 80;

/// 出图失败分类。`code` 与 [`provider_config::status_class`] 同一套词，另加配置类
/// `unsupported_provider`（非 OpenAI 兼容）/ `provider_missing` / `missing_credential`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFailure {
    pub code: &'static str,
    pub status: Option<u16>,
}

impl ImageFailure {
    fn code(code: &'static str) -> Self {
        Self { code, status: None }
    }
    fn request(e: &ureq::Error) -> Self {
        match e {
            ureq::Error::StatusCode(s) => Self {
                code: provider_config::status_class(*s),
                status: Some(*s),
            },
            ureq::Error::Timeout(_) => Self::code("deadline"),
            ureq::Error::BadUri(_) => Self::code("invalid_url"),
            _ => Self::code("transport"),
        }
    }
}

/// 生成结果：规整后的 PNG data URL（正方形、[`AVATAR_SIDE`] 边长）+ 供应商报的用量。
#[derive(Debug, Clone)]
pub struct Generated {
    pub data_url: String,
    /// (输入 token, 输出 token)；供应商不报（dall-e 系）为 None → 账上记未知费用。
    pub usage: Option<(u64, u64)>,
}

/// 工具上下文里的出图通道：只封装钥匙仓，工具拿不到钥匙本身。
#[derive(Clone)]
pub struct ImageAccess {
    creds: Arc<dyn CredentialStore>,
}

impl ImageAccess {
    pub fn new(creds: Arc<dyn CredentialStore>) -> Self {
        Self { creds }
    }

    /// 解析出图槽的供应商端点。能力判定（模型能不能出图）在调用方用
    /// `image_generation_slot` 先做；这里只管「能不能发」。
    pub fn endpoint(&self, doc: &ProviderDoc) -> Result<ImageEndpoint, ImageFailure> {
        let binding = provider_config::resolve_exact(&doc.slots, provider_config::IMAGE_SLOT)
            .ok_or(ImageFailure::code("provider_missing"))?;
        let def = doc
            .providers
            .iter()
            .find(|p| p.id == binding.provider_id && p.enabled)
            .ok_or(ImageFailure::code("provider_missing"))?;
        if def.kind != ProviderKind::OpenAi {
            return Err(ImageFailure::code("unsupported_provider"));
        }
        let key = self
            .creds
            .get(&crate::credentials::provider_key_name(&def.id))
            .ok()
            .flatten()
            .filter(|k| !k.is_empty())
            .ok_or(ImageFailure::code("missing_credential"))?;
        Ok(ImageEndpoint {
            url: images_url(&def.base_url),
            model: binding.model.clone(),
            key,
        })
    }
}

/// 一次出图要打的端点。不实现 Debug：里面有钥匙。
pub struct ImageEndpoint {
    url: String,
    pub model: String,
    key: String,
}

/// 存储值 → 出图地址，与对话请求同根（票 21 `provider_url`：先去掉完整端点尾）。
pub fn images_url(stored: &str) -> String {
    let base = stored.trim().trim_end_matches('/');
    let root = base.strip_suffix("/chat/completions").unwrap_or(base);
    format!("{root}/images/generations")
}

/// 职责摘要：第一句（中英文句读），再截到 [`DUTY_SUMMARY_CHARS`]。
pub fn duty_summary(duty: &str) -> String {
    let first = duty
        .split(['。', '；', '\n', '.', ';'])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("");
    first.chars().take(DUTY_SUMMARY_CHARS).collect()
}

/// 画师的「出图提示词」：统一风格前缀 + 角色名 + 职责摘要。全队同一前缀 = 画风一致。
/// 外发内容只有这三样（见模块注释）。
pub fn avatar_prompt(style: &str, role: &str, duty: &str) -> String {
    let summary = duty_summary(duty);
    let what = if summary.is_empty() {
        format!("the team role \"{role}\"")
    } else {
        format!("the team role \"{role}\" ({summary})")
    };
    format!(
        "{style}. A square avatar portrait representing {what}. One centered subject, simple background, no text, no letters, no watermark."
    )
}

/// 发一次出图请求并规整结果。
pub fn generate(ep: &ImageEndpoint, prompt: &str) -> Result<Generated, ImageFailure> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(180)))
        .build()
        .into();
    let body = json!({"model": ep.model, "prompt": prompt, "n": 1, "size": REQUEST_SIZE});
    let mut resp = agent
        .post(&ep.url)
        .header("Authorization", &format!("Bearer {}", ep.key))
        .send_json(&body)
        .map_err(|e| ImageFailure::request(&e))?;
    let v: Value = resp
        .body_mut()
        .with_config()
        .limit(MAX_RAW_BYTES * 2)
        .read_json()
        .map_err(|_| ImageFailure::code("invalid_response"))?;
    let usage = v["usage"]["input_tokens"]
        .as_u64()
        .zip(v["usage"]["output_tokens"].as_u64());
    let first = &v["data"][0];
    let raw = if let Some(b64) = first["b64_json"].as_str() {
        base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|_| ImageFailure::code("invalid_response"))?
    } else if let Some(url) = first["url"].as_str() {
        download(&agent, url)?
    } else {
        return Err(ImageFailure::code("invalid_response"));
    };
    if raw.len() as u64 > MAX_RAW_BYTES {
        return Err(ImageFailure::code("invalid_response"));
    }
    Ok(Generated {
        data_url: square_png(&raw)?,
        usage,
    })
}

/// 结果 URL 下载：只认 http(s)，不带钥匙（可能是另一台主机的对象存储）。
fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, ImageFailure> {
    let parsed = url::Url::parse(url).map_err(|_| ImageFailure::code("invalid_response"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ImageFailure::code("invalid_response"));
    }
    let mut resp = agent
        .get(url)
        .call()
        .map_err(|e| ImageFailure::request(&e))?;
    resp.body_mut()
        .with_config()
        .limit(MAX_RAW_BYTES)
        .read_to_vec()
        .map_err(|_| ImageFailure::code("invalid_response"))
}

/// 解码 → 中心裁方 → 缩到 [`AVATAR_SIDE`] → PNG data URL（之后走票 17 的头像写入路径）。
pub fn square_png(raw: &[u8]) -> Result<String, ImageFailure> {
    let img = image::load_from_memory(raw).map_err(|_| ImageFailure::code("invalid_response"))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(ImageFailure::code("invalid_response"));
    }
    let side = w.min(h);
    let square = img
        .crop_imm((w - side) / 2, (h - side) / 2, side, side)
        .resize_exact(
            AVATAR_SIDE,
            AVATAR_SIDE,
            image::imageops::FilterType::Triangle,
        );
    let mut out = std::io::Cursor::new(Vec::new());
    square
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|_| ImageFailure::code("invalid_response"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_url_shares_the_chat_root() {
        assert_eq!(
            images_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/images/generations"
        );
        assert_eq!(
            images_url("https://x.test/v1/chat/completions/"),
            "https://x.test/v1/images/generations"
        );
    }

    #[test]
    fn prompt_carries_only_style_role_and_first_duty_sentence() {
        let p = avatar_prompt(
            "pixel art",
            "后端",
            "负责服务端接口与数据。与前端对齐契约；不碰界面",
        );
        assert!(p.starts_with("pixel art."));
        assert!(p.contains("后端"));
        assert!(p.contains("负责服务端接口与数据"));
        assert!(!p.contains("与前端对齐契约"), "只取第一句: {p}");
    }

    #[test]
    fn square_png_center_crops_and_downscales() {
        let img = image::DynamicImage::new_rgb8(600, 300);
        let mut raw = std::io::Cursor::new(Vec::new());
        img.write_to(&mut raw, image::ImageFormat::Png).unwrap();
        let url = square_png(&raw.into_inner()).unwrap();
        let b64 = url.strip_prefix("data:image/png;base64,").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!((out.width(), out.height()), (AVATAR_SIDE, AVATAR_SIDE));
    }
}
