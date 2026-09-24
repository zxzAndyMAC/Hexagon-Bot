//! 界面语言进核心（prompt-engineering 票 06 / ADR 0071）。
//!
//! 面向模型的固定提示词统一英文，回复语言跟随负责人的界面语言。界面语言
//! 原先只在前端 `localStorage`（`hexagon.lang`），核心拿不到；前端切换语言
//! （以及启动时）经壳层写进宿主文件 `~/.hexagon/ui.json`，回合开始时读取。
//! 未设置（场景回放、无界面运行）回落英文。
//!
//! 产物语言不跟这里走——跟项目说明或同阶段已有产物（spec Q21 裁决：切界面
//! 语言不该让同一仓库的产物语言混杂）。

use serde_json::json;
use std::path::{Path, PathBuf};

/// 与前端 `SUPPORTED` 同一集合。
pub const SUPPORTED: [&str; 7] = ["en", "zh-CN", "zh-TW", "ja", "es", "pt", "fr"];

#[derive(Debug, thiserror::Error)]
pub enum UiLangError {
    #[error("unsupported interface language: {0}")]
    Unsupported(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// 宿主文件路径（`HEXAGON_UI_PATH` 覆盖）。cfg(test) 下指向不存在的路径：
/// 并行测试不能靠环境变量隔离，也不能读开发者本机的设置——单测走 `*_at`。
fn runtime_path() -> PathBuf {
    #[cfg(test)]
    {
        PathBuf::from("/nonexistent/hexagon-test/ui.json")
    }
    #[cfg(not(test))]
    {
        if let Ok(p) = std::env::var("HEXAGON_UI_PATH") {
            return PathBuf::from(p);
        }
        std::env::var_os("HOME")
            .map(|h| Path::new(&h).join(".hexagon/ui.json"))
            .unwrap_or_else(|| PathBuf::from("/nonexistent/.hexagon/ui.json"))
    }
}

pub fn set_language(code: &str) -> Result<(), UiLangError> {
    set_language_at(&runtime_path(), code)
}

pub fn language() -> Option<String> {
    language_at(&runtime_path())
}

pub fn set_language_at(path: &Path, code: &str) -> Result<(), UiLangError> {
    if !SUPPORTED.contains(&code) {
        return Err(UiLangError::Unsupported(code.into()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, json!({ "language": code }).to_string())?;
    Ok(())
}

/// 文件缺失、损坏或值不在支持集里都按未设置处理（读面容忍）。
pub fn language_at(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let code = v["language"].as_str()?;
    SUPPORTED.contains(&code).then(|| code.to_string())
}

/// locale 码 → 写进提示词的语言名。未设置回落英文。
pub fn language_name(code: Option<&str>) -> &'static str {
    match code {
        Some("zh-CN") => "Simplified Chinese",
        Some("zh-TW") => "Traditional Chinese",
        Some("ja") => "Japanese",
        Some("es") => "Spanish",
        Some("pt") => "Portuguese",
        Some("fr") => "French",
        _ => "English",
    }
}

#[cfg(test)]
thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// 回合读取点：当前界面语言的语言名。
pub fn reply_language() -> &'static str {
    #[cfg(test)]
    {
        if let Some(c) = TEST_OVERRIDE.with(|o| o.borrow().clone()) {
            return language_name(Some(&c));
        }
    }
    language_name(language().as_deref())
}

/// 测试缝：本线程的回合看到指定界面语言（运行时路径在测试构建下不存在）。
#[cfg(test)]
pub fn set_test_language(code: Option<&str>) {
    TEST_OVERRIDE.with(|o| *o.borrow_mut() = code.map(Into::into));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_back() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/ui.json");
        set_language_at(&p, "zh-CN").unwrap();
        assert_eq!(language_at(&p).as_deref(), Some("zh-CN"));
    }

    #[test]
    fn unsupported_code_rejected_and_nothing_written() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ui.json");
        assert!(matches!(
            set_language_at(&p, "de"),
            Err(UiLangError::Unsupported(_))
        ));
        assert!(!p.exists());
    }

    #[test]
    fn missing_or_corrupt_file_reads_as_unset() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ui.json");
        assert_eq!(language_at(&p), None);
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(language_at(&p), None);
        std::fs::write(&p, r#"{"language":"klingon"}"#).unwrap();
        assert_eq!(language_at(&p), None);
    }

    #[test]
    fn unset_falls_back_to_english() {
        assert_eq!(language_name(None), "English");
        assert_eq!(language_name(Some("ja")), "Japanese");
        set_test_language(None);
        assert_eq!(reply_language(), "English");
        set_test_language(Some("zh-TW"));
        assert_eq!(reply_language(), "Traditional Chinese");
        set_test_language(None);
    }
}
