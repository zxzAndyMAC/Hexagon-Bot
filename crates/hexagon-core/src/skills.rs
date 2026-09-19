//! 技能层（openworker-borrow 票 09）：SKILL.md 渐进披露。
//!
//! 出处：OpenWorker `coworker/skills/{base,store}.py`（registry/store/mute
//! 三件套结构照搬）。一个技能 = 一个含 `SKILL.md` 的目录
//! （YAML frontmatter：name/description + 正文指令 + 可选资源文件）。
//!
//! 渐进披露：系统提示只注入一行式 catalog（`- name: desc`），全文经
//! `load_skill` 只读工具按需取——窄上下文只装指针，「接口设计规范」
//! 「测试约定」这类知识不再膨胀每个角色提示。
//!
//! 目录优先级：项目 `.hexagon/skills/` > 全局 `~/.hexagon/skills/`
//! （全局先扫，同名被项目覆盖——就近原则）。
//!
//! 边界（防自授）：skills 目录是负责人管理的提示词内容，agent 写入被
//! `tools::is_agent_policy_path` 硬拒——写进 SKILL.md = 给自己追加
//! 指令，与 permission_rules 同类。

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// 技能名长度上限（store.py `_MAX_NAME` 同值）。
const MAX_NAME: usize = 64;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// 完整正文——按需加载，不进系统提示。
    pub instructions: String,
    /// 技能目录（资源/脚本所在），agent 可按需 fs_read。
    pub path: PathBuf,
}

/// 技能名 = 目录名：只允许字母数字点线，禁 `..`/`/`/`\`——
/// 拒绝一切能逃出 scope 目录的形态（store.py `validate_name`）。
pub fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("skill name is required".into());
    }
    if name.len() > MAX_NAME {
        return Err(format!("skill name too long (limit {MAX_NAME})"));
    }
    if name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(
            "skill name may only contain letters, digits, dots, dashes, underscores".into(),
        );
    }
    Ok(name.to_string())
}

/// 加载器：构造即扫描；miss 时 rescan（会话中新建的技能也能取到——
/// catalog 行静止到下一会话，但显式点名不许 404）。
pub struct SkillLoader {
    dirs: Vec<PathBuf>,
    skills: BTreeMap<String, Skill>,
}

impl SkillLoader {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        let mut l = Self {
            dirs,
            skills: BTreeMap::new(),
        };
        l.rescan();
        l
    }

    pub fn rescan(&mut self) {
        self.skills.clear();
        // 顺序即优先级：先扫的被后扫的覆盖（全局 → 项目）。
        for dir in self.dirs.clone() {
            self.discover(&dir);
        }
    }

    fn discover(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut subs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subs.sort();
        for sub in subs {
            let md = sub.join("SKILL.md");
            if md.is_file() {
                if let Some(s) = parse_skill(&md) {
                    self.skills.insert(s.name.clone(), s);
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// 可用的技能名（扣掉本会话 mute 的）。
    pub fn visible_names(&self, muted: &HashSet<String>) -> Vec<String> {
        self.skills
            .keys()
            .filter(|n| !muted.contains(*n))
            .cloned()
            .collect()
    }

    /// 一行式 catalog；空目录/全 mute → None（不占提示词）。
    pub fn catalog_text(&self, muted: &HashSet<String>) -> Option<String> {
        let lines: Vec<String> = self
            .skills
            .values()
            .filter(|s| !muted.contains(&s.name))
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect();
        if lines.is_empty() {
            return None;
        }
        Some(format!(
            "可用技能 —— 与当前任务相关时调用 load_skill(name) 取全文：\n{}",
            lines.join("\n")
        ))
    }
}

/// frontmatter 解析：只认 `---` 头块里的 name/description，
/// 缺 name 用目录名兜底（与 OpenWorker `_parse_skill` 同语义）。
fn parse_skill(md: &Path) -> Option<Skill> {
    let text = std::fs::read_to_string(md).ok()?;
    let mut name = md
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let (mut description, mut body) = (String::new(), text.as_str());
    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            body = rest[end + 4..].trim_start_matches('\n');
            for line in fm.lines() {
                let Some((k, v)) = line.split_once(':') else {
                    continue;
                };
                match k.trim().to_ascii_lowercase().as_str() {
                    "name" if !v.trim().is_empty() => name = v.trim().to_string(),
                    "description" => description = v.trim().to_string(),
                    _ => {}
                }
            }
        }
    }
    if name.is_empty() || validate_name(&name).is_err() {
        return None;
    }
    Some(Skill {
        name,
        description,
        instructions: body.trim().to_string(),
        path: md.parent()?.to_path_buf(),
    })
}

/// 全局技能目录：`~/.hexagon/skills/`。
fn global_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| Path::new(&h).join(".hexagon/skills"))
}

/// 扫描顺序：全局先、项目后（项目覆盖全局同名）。
pub fn skill_dirs(repo_root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = global_dir().into_iter().collect();
    v.push(repo_root.join(".hexagon/skills"));
    v
}

/// 会话级 mute 集：`.hexagon/local/skill-mutes.json`
/// （`{session: {skill: enabled}}`；缺项 = 继承启用）。
/// 独立 settings 文件——`.hexagon/local/` 写入即带 `.gitignore`，
/// 偏好不随产物进 git（产物进 git 是特性，会话偏好不是）。
pub struct SkillMutes {
    path: PathBuf,
    rows: BTreeMap<String, BTreeMap<String, bool>>,
}

impl SkillMutes {
    pub fn load(repo_root: &Path) -> Self {
        let path = repo_root.join(".hexagon/local/skill-mutes.json");
        let rows = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| {
                v["sessions"].as_object().map(|o| {
                    o.iter()
                        .map(|(sid, m)| {
                            (
                                sid.clone(),
                                m.as_object()
                                    .map(|mm| {
                                        mm.iter()
                                            .map(|(k, v)| (k.clone(), v.as_bool().unwrap_or(true)))
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                            )
                        })
                        .collect()
                })
            })
            .unwrap_or_default();
        Self { path, rows }
    }

    /// 该会话被 mute 的技能名集合（值为 false 的）。
    pub fn muted_set(&self, session: &str) -> HashSet<String> {
        self.rows
            .get(session)
            .map(|m| {
                m.iter()
                    .filter(|(_, on)| !**on)
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set(&mut self, session: &str, skill: &str, enabled: bool) -> std::io::Result<()> {
        self.rows
            .entry(session.to_string())
            .or_default()
            .insert(skill.to_string(), enabled);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
            // 本地偏好不进 git——同目录写 .gitignore 兜底（幂等）。
            let gi = dir.join(".gitignore");
            if !gi.exists() {
                std::fs::write(&gi, "*\n")?;
            }
        }
        let data = serde_json::json!({ "sessions": self.rows });
        std::fs::write(&self.path, serde_json::to_string_pretty(&data)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(dir: &Path, name: &str, desc: &str, body: &str) {
        let d = dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {desc}\n---\n{body}"),
        )
        .unwrap();
    }

    #[test]
    fn loader_finds_parses_and_catalogs() {
        let root = tempfile::tempdir().unwrap();
        let skills = root.path().join("s");
        skill(&skills, "api-conv", "接口设计规范", "正文：先写契约。");
        let mut l = SkillLoader::new(vec![skills]);
        assert_eq!(l.get("api-conv").unwrap().instructions, "正文：先写契约。");
        let text = l.catalog_text(&HashSet::new()).unwrap();
        assert!(text.contains("- api-conv: 接口设计规范"));
        // miss 后 rescan：会话中新建的能取到
        skill(&root.path().join("s"), "new-one", "n", "b");
        assert!(l.get("new-one").is_none());
        l.rescan();
        assert!(l.get("new-one").is_some());
    }

    #[test]
    fn project_overrides_global_same_name() {
        let root = tempfile::tempdir().unwrap();
        let g = root.path().join("g");
        let p = root.path().join("p");
        skill(&g, "same", "global版", "G");
        skill(&p, "same", "project版", "P");
        let l = SkillLoader::new(vec![g, p]);
        assert_eq!(l.get("same").unwrap().description, "project版");
        assert_eq!(l.get("same").unwrap().instructions, "P");
    }

    #[test]
    fn name_validation_rejects_escape() {
        for bad in [
            "../x",
            "a/b",
            "a\\b",
            "..",
            "",
            &"x".repeat(65),
            "a b",
            "a;b",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} must be rejected");
        }
        for ok in ["api-conv", "test_v2", "x.y", "ABC"] {
            assert!(validate_name(ok).is_ok(), "{ok:?} must be accepted");
        }
    }

    #[test]
    fn mutes_hide_from_catalog_and_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        let mut m = SkillMutes::load(root.path());
        m.set("sr1", "api-conv", false).unwrap();
        // 持久化 + gitignore 兜底
        let m2 = SkillMutes::load(root.path());
        assert!(m2.muted_set("sr1").contains("api-conv"));
        assert!(!m2.muted_set("sr2").contains("api-conv"));
        assert!(root.path().join(".hexagon/local/.gitignore").exists());
        // catalog 过滤
        let skills = root.path().join("s");
        skill(&skills, "api-conv", "d", "b");
        skill(&skills, "other", "d2", "b2");
        let l = SkillLoader::new(vec![skills]);
        let text = l.catalog_text(&m2.muted_set("sr1")).unwrap();
        assert!(!text.contains("api-conv"));
        assert!(text.contains("other"));
    }
}
