//! 功能缺口 A7（2026-10-15，decisions#design 底栏「沿用你上次的视觉指导」）：负责人上一次亲手确认的视觉指导，
//! 跨项目保留在 `~/.hexagon/visual-guidance.json`（与 `skill-mutes.json` 同一处负责人级持久文件）。
//!
//! - 只记负责人本人的确认（`by == "owner"`）：托管代选不是「你的」指导，记了会让信号冒充负责人意见。
//! - 新项目方向未定时，读出来进 D12 卡（信号）和角色的设计简报（让提案真的沿用它），方向一经确认就不再给。
//! - 文件读不出 / 损坏 = 没有上次指导：缺信号花负责人一次重复说明，造信号会把别的项目的风格硬套过来。
//! - 测试进程默认不落盘（HOME 是开发者真实目录），只认线程内注入的路径。
use std::path::PathBuf;

/// 指导文本上限：与 choose 的 existing_guidance 上限同口径。
const MAX_TEXT: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PreviousGuidance {
    /// `selected`（选了某个方案）| `existing`（给了已有的品牌 / 设计规范）。
    #[ts(type = "'selected' | 'existing'")]
    pub source: String,
    /// 选中方案的标题；existing 为 null。
    pub title: Option<String>,
    /// 方案说明 + 布局 / 字体 / 配色，或负责人给的规范原文（截断到 4000 字符）。
    pub summary: String,
    pub recorded_at: String,
}

#[cfg(not(test))]
fn path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HEXAGON_VISUAL_GUIDANCE_PATH") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".hexagon/visual-guidance.json"))
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn path() -> Option<PathBuf> {
    TEST_PATH.with(|p| p.borrow().clone())
}

pub fn last() -> Option<PreviousGuidance> {
    let text = std::fs::read_to_string(path()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// 负责人确认方向后记一笔；失败只记日志，不影响已提交的选择。
pub(super) fn remember(direction: &super::DesignDirection) {
    let Some(entry) = entry(direction) else {
        return;
    };
    let Some(path) = path() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(&path))?;
        std::io::Write::write_all(&mut tmp, serde_json::to_string(&entry)?.as_bytes())?;
        tmp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    };
    if let Err(error) = write() {
        log::warn!("visual guidance not remembered: {error}");
    }
}

fn entry(direction: &super::DesignDirection) -> Option<PreviousGuidance> {
    let clip = |text: String| text.chars().take(MAX_TEXT).collect::<String>();
    let recorded_at = direction.selected_at.clone()?;
    match direction.state.as_str() {
        "selected" => {
            let option = direction
                .options
                .iter()
                .find(|o| Some(&o.id) == direction.selected_option.as_ref())?;
            Some(PreviousGuidance {
                source: "selected".into(),
                title: Some(option.title.clone()),
                summary: clip(format!(
                    "{}\nlayout: {}\ntypography: {}\npalette: {}",
                    option.description, option.layout, option.typography, option.palette
                )),
                recorded_at,
            })
        }
        "existing" => Some(PreviousGuidance {
            source: "existing".into(),
            title: None,
            summary: clip(direction.existing_guidance.clone()?),
            recorded_at,
        }),
        _ => None,
    }
}
