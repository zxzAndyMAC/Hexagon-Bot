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
/// 三层就近优先：内置预置 < 全局 < 项目（内置是最底层种子——此前
/// presets/skills 嵌二进制却不在 loader 里，角色默认技能名全 404；
/// global-config 票 03 修）。
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
        // 内置层：path 为空路径作标记（listing 判 origin="builtin"）。
        for (name, text) in crate::presets::SKILL_FILES {
            if let Some(s) = parse_skill_text(text, name, PathBuf::new()) {
                l.skills.insert(s.name.clone(), s);
            }
        }
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
    let fallback = md
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    parse_skill_text(&text, fallback, md.parent()?.to_path_buf())
}

/// 正文解析（内置技能从常量字符串来，`path` 传空路径作 builtin 标记）。
fn parse_skill_text(text: &str, fallback_name: &str, path: PathBuf) -> Option<Skill> {
    let mut name = fallback_name.to_string();
    let (mut description, mut body) = (String::new(), text);
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
        path,
    })
}

/// owner 级静音的遗留会话键（ui-audit-2 票 04）：早期版本把设置页开关写进
/// 项目 `.hexagon/local/skill-mutes.json` 的 `"*"` 会话键——那只作用当前项目，
/// 语义是假的「全局」。ADR 0057 起真全局静音集落 `~/.hexagon/skill-mutes.json`；
/// 遗留 `"*` 行仍被读（不丢存量用户开关），但范围退化为所在项目。
pub const GLOBAL_MUTE_SESSION: &str = "*";

/// 全局静音集文件：`~/.hexagon/skill-mutes.json`（`{"muted":[name]}` 平表——
/// 无会话维度，就是 owner 的持久开关）。
fn global_mutes_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HEXAGON_SKILL_MUTES_PATH") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("HOME").map(|h| Path::new(&h).join(".hexagon/skill-mutes.json"))
}

/// 读指定静音文件（读失败=空——损坏文件不应让全部技能消失或炸掉回合）。
fn global_muted_set_at(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| {
            v["muted"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

fn set_muted_at(path: &Path, skill: &str, enabled: bool) -> std::io::Result<()> {
    let mut set = global_muted_set_at(path);
    if enabled {
        set.remove(skill);
    } else {
        set.insert(skill.to_string());
    }
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // tmp+rename：半截写不留下损坏静音文件（同 provider_config::write_doc）。
    let tmp = path.with_extension("tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&serde_json::json!({ "muted": list }))?,
    )?;
    std::fs::rename(&tmp, path)
}

/// 设置页开关写全局文件（任何项目即刻生效）。
pub fn set_globally_muted(skill: &str, enabled: bool) -> std::io::Result<()> {
    let p = global_mutes_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no HOME for global mutes")
    })?;
    set_muted_at(&p, skill, enabled)
}

/// 回合/工具面统一取法：会话 mute ∪ 遗留 "*" ∪ 全局文件（并集而非覆盖）。
pub fn effective_muted_at(repo_root: &Path, session: &str, global_path: &Path) -> HashSet<String> {
    let mutes = SkillMutes::load(repo_root);
    let mut muted = mutes.muted_set(session);
    muted.extend(mutes.muted_set(GLOBAL_MUTE_SESSION));
    muted.extend(global_muted_set_at(global_path));
    muted
}

pub fn effective_muted(repo_root: &Path, session: &str) -> HashSet<String> {
    match global_mutes_path() {
        Some(p) => effective_muted_at(repo_root, session, &p),
        None => {
            let mutes = SkillMutes::load(repo_root);
            let mut muted = mutes.muted_set(session);
            muted.extend(mutes.muted_set(GLOBAL_MUTE_SESSION));
            muted
        }
    }
}

/// 全局技能清单（无项目态）：只扫 ~/.hexagon/skills，enabled 看全局集。
pub fn list_global() -> Vec<SkillRow> {
    let muted = global_mutes_path()
        .map(|p| global_muted_set_at(&p))
        .unwrap_or_default();
    let loader = SkillLoader::new(global_dir().into_iter().collect());
    loader
        .skills
        .values()
        .map(|s| SkillRow {
            name: s.name.clone(),
            description: s.description.clone(),
            origin: if s.path.as_os_str().is_empty() {
                "builtin".into()
            } else {
                "global".into()
            },
            enabled: !muted.contains(&s.name),
        })
        .collect()
}

/// 在指定技能目录写 <name>/SKILL.md（frontmatter 标准形）。
/// validate_name 挡一切路径逃逸形态。
pub fn save_skill_at(
    skills_dir: &Path,
    name: &str,
    description: &str,
    body: &str,
) -> Result<(), std::io::Error> {
    let name = validate_name(name).map_err(std::io::Error::other)?;
    let dir = skills_dir.join(&name);
    std::fs::create_dir_all(&dir)?;
    let md = format!(
        "---\nname: {name}\ndescription: {}\n---\n\n{}\n",
        description.trim(),
        body.trim()
    );
    std::fs::write(dir.join("SKILL.md"), md)
}

/// 新建/覆写全局技能（设置页提交）：~/.hexagon/skills/<name>。
pub fn save_global_skill(name: &str, description: &str, body: &str) -> Result<(), std::io::Error> {
    let dir = global_dir().ok_or_else(|| std::io::Error::other("no HOME for global skills"))?;
    save_skill_at(&dir, name, description, body)
}

/// 技能解析（合并视图里按名；project 优先全局；内置为种子层）。
fn skill_of(name: &str, repo_root: Option<&Path>) -> Option<Skill> {
    let dirs = match repo_root {
        Some(root) => skill_dirs(root),
        None => global_dir().into_iter().collect(),
    };
    let loader = SkillLoader::new(dirs);
    loader.get(name).cloned()
}

/// 技能包文件清单（详情面文件树）：有界遍历——≤400 条目、不跟 symlink、
/// 跳隐藏文件。内置技能无物理目录（path 空标记），合成 ["SKILL.md"]。
/// 防逃逸靠「遍历起点就是技能目录 + 相对路径返回」。
pub fn skill_files(name: &str, repo_root: Option<&Path>) -> Vec<String> {
    let Some(skill) = skill_of(name, repo_root) else {
        return Vec::new();
    };
    if skill.path.as_os_str().is_empty() {
        return vec!["SKILL.md".to_string()];
    }
    let base = skill.path;
    let mut out = Vec::new();
    let mut stack = vec![base.clone()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            if out.len() >= 400 {
                return out;
            }
            let p = e.path();
            let Ok(rel) = p.strip_prefix(&base) else {
                continue;
            };
            let rel = rel.to_string_lossy().to_string();
            if rel.starts_with('.') || rel.contains("/.") {
                continue;
            }
            // 不跟 symlink：symlink_metadata 而非 is_dir 判型
            let Ok(md) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if md.is_dir() {
                stack.push(p);
            } else {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// 读技能包内文件（详情渲染）：rel 不得含 `..`/绝对路径；规范路径必须仍在
/// 技能目录内；正文截 256KB——技能包不该装巨文件，真装了也只给头部。
pub fn read_skill_file(
    name: &str,
    rel: &str,
    repo_root: Option<&Path>,
) -> Result<String, std::io::Error> {
    let skill =
        skill_of(name, repo_root).ok_or_else(|| std::io::Error::other("skill not found"))?;
    if rel.contains("..") || rel.starts_with('/') || rel.starts_with('\\') {
        return Err(std::io::Error::other("path escape rejected"));
    }
    // 内置技能无物理目录——SKILL.md 由 frontmatter+正文回构。
    if skill.path.as_os_str().is_empty() {
        return if rel == "SKILL.md" {
            Ok(format!(
                "---\nname: {}\ndescription: {}\n---\n\n{}\n",
                skill.name, skill.description, skill.instructions
            ))
        } else {
            Err(std::io::Error::other("builtin skill has no extra files"))
        };
    }
    let base = skill.path;
    let p = base.join(rel);
    let canon_base = base.canonicalize()?;
    let canon_p = p.canonicalize()?;
    if !canon_p.starts_with(&canon_base) {
        return Err(std::io::Error::other("path escape rejected"));
    }
    let bytes = std::fs::read(&p)?;
    let cap = bytes.len().min(256 * 1024);
    Ok(String::from_utf8_lossy(&bytes[..cap]).to_string())
}

/// 设置-技能分区行：发现结果 + 全局静音态。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SkillRow {
    pub name: String,
    pub description: String,
    /// global = ~/.hexagon/skills；project = <repo>/.hexagon/skills
    pub origin: String,
    /// 全局开关态（`"*"` 会话键的 mute 集）。
    pub enabled: bool,
}

/// 扫描目录 + 静音态（设置页列表用；同名项目技能覆盖全局）。
/// enabled = !(全局文件 ∪ 遗留 "*" 项目行)——会话 mute 是回合内状态，
/// 不该让设置页开关显示瞬时态。
pub fn list_all(repo_root: &Path) -> Vec<SkillRow> {
    list_all_at(repo_root, global_mutes_path().as_deref())
}

pub fn list_all_at(repo_root: &Path, global_mutes: Option<&Path>) -> Vec<SkillRow> {
    let loader = SkillLoader::new(skill_dirs(repo_root));
    let mut muted = global_mutes.map(global_muted_set_at).unwrap_or_default();
    muted.extend(SkillMutes::load(repo_root).muted_set(GLOBAL_MUTE_SESSION));
    let gdir = global_dir();
    loader
        .skills
        .values()
        .map(|s| SkillRow {
            name: s.name.clone(),
            description: s.description.clone(),
            origin: if s.path.as_os_str().is_empty() {
                "builtin".into()
            } else if gdir.as_ref().is_some_and(|g| s.path.starts_with(g)) {
                "global".into()
            } else {
                "project".into()
            },
            enabled: !muted.contains(&s.name),
        })
        .collect()
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

    /// ui-audit-2 票 04 → global-config 票 03：全局静音迁
    /// `~/.hexagon/skill-mutes.json`（真全局）；遗留 "*" 项目行仍生效。
    #[test]
    fn list_all_and_global_mute() {
        let root = tempfile::tempdir().unwrap();
        let gmutes = root.path().join("gmutes.json");
        skill(
            &root.path().join(".hexagon/skills"),
            "api-conv",
            "d",
            "body",
        );
        let row = list_all_at(root.path(), Some(&gmutes))
            .into_iter()
            .find(|r| r.name == "api-conv")
            .unwrap();
        assert!(row.enabled);
        assert_eq!(row.origin, "project");
        // 全局文件静音 → 列表 disable；再开回 → enabled
        set_muted_at(&gmutes, "api-conv", false).unwrap();
        assert!(
            !list_all_at(root.path(), Some(&gmutes))
                .into_iter()
                .find(|r| r.name == "api-conv")
                .unwrap()
                .enabled
        );
        set_muted_at(&gmutes, "api-conv", true).unwrap();
        assert!(
            list_all_at(root.path(), Some(&gmutes))
                .into_iter()
                .find(|r| r.name == "api-conv")
                .unwrap()
                .enabled
        );
        // 遗留 "*" 行（旧版全局开关的项目内落点）仍生效
        let mut m = SkillMutes::load(root.path());
        m.set(GLOBAL_MUTE_SESSION, "api-conv", false).unwrap();
        assert!(effective_muted_at(root.path(), "any-session", &gmutes).contains("api-conv"));
        assert!(root
            .path()
            .join(".hexagon/local/skill-mutes.json")
            .is_file());
        assert!(root.path().join(".hexagon/local/.gitignore").is_file());
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

    /// global-config 票 03：设置页「新建技能」写 <dir>/<name>/SKILL.md
    /// 标准 frontmatter 形；坏名拒绝不落盘。
    #[test]
    fn save_skill_writes_loadable_md() {
        let dir = tempfile::tempdir().unwrap();
        save_skill_at(dir.path(), "my-skill", "描述一", "正文内容").unwrap();
        let l = SkillLoader::new(vec![dir.path().to_path_buf()]);
        let s = l.get("my-skill").unwrap();
        assert_eq!(s.description, "描述一");
        assert_eq!(s.instructions, "正文内容");
        assert!(save_skill_at(dir.path(), "../evil", "d", "b").is_err());
        assert!(save_skill_at(dir.path(), "a b", "d", "b").is_err());
    }

    /// 技能包文件树 + 读文件：有界、拒逃逸、不列隐藏文件。
    /// （repo_root 注入路径——None 分支走真 HOME，测试不碰。）
    #[test]
    fn skill_files_and_read_bounded() {
        let root = tempfile::tempdir().unwrap();
        let prj = root.path().join(".hexagon/skills/sk");
        std::fs::create_dir_all(prj.join("scripts")).unwrap();
        std::fs::write(prj.join("SKILL.md"), "---\nname: sk\n---\nbody").unwrap();
        std::fs::write(prj.join("scripts/x.py"), "print(1)").unwrap();
        std::fs::write(prj.join(".hidden"), "x").unwrap();
        let files = skill_files("sk", Some(root.path()));
        assert_eq!(files, vec!["SKILL.md", "scripts/x.py"]);
        assert!(read_skill_file("sk", "scripts/x.py", Some(root.path()))
            .unwrap()
            .contains("print"));
        assert!(read_skill_file("sk", "../AGENTS.md", Some(root.path())).is_err());
        assert!(read_skill_file("sk", "/etc/passwd", Some(root.path())).is_err());
        assert!(files.iter().all(|f| !f.starts_with('.')));
    }

    /// 票 04：外部扫描去重 + 来源标注 + 只认含 SKILL.md 的目录。
    #[test]
    fn scan_external_dedup_by_name() {
        let home = tempfile::tempdir().unwrap();
        skill(&home.path().join(".cursor/skills"), "zz-a", "cursor版", "b");
        skill(&home.path().join(".claude/skills"), "zz-a", "claude版", "b");
        skill(
            &home.path().join(".claude/skills"),
            "zz-b",
            "claude独苗",
            "b",
        );
        std::fs::create_dir_all(home.path().join(".agents/skills/zz-c")).unwrap(); // 无 SKILL.md
        let rows = scan_external_skills_at(home.path());
        assert_eq!(rows.len(), 2);
        let a = rows.iter().find(|r| r.name == "zz-a").unwrap();
        assert_eq!(a.origin, "cursor"); // 表序 = 优先级，先扫到的赢
        assert_eq!(a.description, "cursor版");
        assert!(!a.conflict);
    }

    /// 票 04：导入复制目录；同名冲突与坏名跳过不炸整批；symlink 不复制。
    #[test]
    fn import_skills_copy_and_skip() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        skill(&home.path().join("ext"), "ok-one", "d", "b");
        std::fs::create_dir_all(dest.path().join("dup")).unwrap();
        std::fs::write(dest.path().join("dup/SKILL.md"), "---\nname: dup\n---\nx").unwrap();
        skill(&home.path().join("ext"), "dup", "d", "b");
        let rep = import_skills_at(
            dest.path(),
            &[
                home.path().join("ext/ok-one").to_string_lossy().to_string(),
                home.path().join("ext/dup").to_string_lossy().to_string(),
                home.path()
                    .join("ext/missing")
                    .to_string_lossy()
                    .to_string(),
            ],
        );
        assert_eq!(rep.imported, 1);
        assert_eq!(rep.skipped.len(), 2);
        assert!(dest.path().join("ok-one/SKILL.md").is_file());
    }

    /// 票 04：ZIP 安装——解包到顶层 SKILL.md 所在目录，../ 成员名被
    /// enclosed_name 拒掉；文件夹路径同语义。
    #[test]
    fn install_skill_zip_and_dir() {
        let dest = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        // 造 zip：wrap/my-skill/SKILL.md + 一个逃逸成员
        let zp = work.path().join("pkg.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
            let opt = zip::write::SimpleFileOptions::default();
            w.add_directory("wrap/my-skill/", opt).unwrap();
            w.start_file("wrap/my-skill/SKILL.md", opt).unwrap();
            use std::io::Write;
            w.write_all(b"---\nname: my-skill\ndescription: z\n---\nzip body")
                .unwrap();
            w.start_file("wrap/my-skill/scripts/x.sh", opt).unwrap();
            w.write_all(b"echo hi").unwrap();
            w.finish().unwrap();
        }
        let name = install_skill_from_path_at(dest.path(), zp.to_str().unwrap()).unwrap();
        assert_eq!(name, "my-skill");
        assert!(dest.path().join("my-skill/SKILL.md").is_file());
        assert!(dest.path().join("my-skill/scripts/x.sh").is_file());
        // 同名再装 = 冲突拒绝
        assert!(install_skill_from_path_at(dest.path(), zp.to_str().unwrap()).is_err());
        // 文件夹路径
        let d = work.path().join("dirskill");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), "---\nname: dirskill\n---\nb").unwrap();
        assert_eq!(
            install_skill_from_path_at(dest.path(), d.to_str().unwrap()).unwrap(),
            "dirskill"
        );
        // 非技能目录拒绝
        let empty = work.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(install_skill_from_path_at(dest.path(), empty.to_str().unwrap()).is_err());
    }
}

// ---------- 外部技能扫描导入（global-config 票 04） ----------
// 探测表数据驱动：各平台技能目录（含 SKILL.md 的目录即技能）。平台缺
// 目录就不出现——不硬凑。chatgpt/grok 无本机技能落盘约定，不在表内。

/// (平台名, HOME 相对路径)
const EXT_SKILL_SOURCES: &[(&str, &str)] = &[
    ("cursor", ".cursor/skills"),
    ("claude", ".claude/skills"),
    ("agents", ".agents/skills"),
    ("devin", ".config/devin/skills"),
    ("windsurf", ".codeium/windsurf/skills"),
    ("codex", ".codex/skills"),
];

/// 外部技能扫描行：去重键 = 技能目录名；conflict = 与本机全局同名
/// （导入时跳过不覆盖——owner 的已有技能优先）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExtSkillRow {
    pub name: String,
    pub description: String,
    /// 来源平台（cursor/claude/agents/devin/windsurf/codex）
    pub origin: String,
    pub path: String,
    pub conflict: bool,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ImportReport {
    pub imported: u32,
    /// 跳过原因串（同名冲突/坏名/读失败），UI toast 汇总用
    pub skipped: Vec<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// 扫外部技能（`home` 注入便于测试；线上用 `home_dir()`）。
/// 全局技能目录同步扫描以判 conflict；全局目录本身不作来源。
pub fn scan_external_skills_at(home: &Path) -> Vec<ExtSkillRow> {
    let existing: HashSet<String> = global_dir()
        .and_then(|g| {
            std::fs::read_dir(&g).ok().map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_str().map(String::from))
                    .collect()
            })
        })
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (platform, rel) in EXT_SKILL_SOURCES {
        let dir = home.join(rel);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if !p.join("SKILL.md").is_file() {
                continue;
            }
            let Some(name) = e.file_name().to_str().map(String::from) else {
                continue;
            };
            if !seen.insert(name.clone()) {
                continue; // 同名去重：先扫到的赢（表序即平台优先级）
            }
            let desc = parse_skill(&p.join("SKILL.md"))
                .map(|s| s.description)
                .unwrap_or_default();
            out.push(ExtSkillRow {
                conflict: existing.contains(&name),
                name,
                description: desc,
                origin: platform.to_string(),
                path: p.to_string_lossy().to_string(),
            });
        }
    }
    out
}

pub fn scan_external_skills() -> Vec<ExtSkillRow> {
    home_dir()
        .map(|h| scan_external_skills_at(&h))
        .unwrap_or_default()
}

/// 有界复制：≤200 文件、≤8MB 总量、symlink 只跳不跟（防止把包外文件
/// 链进技能目录）。dst 存在即冲突跳过。
fn copy_skill_dir(src: &Path, dst: &Path) -> Result<u32, std::io::Error> {
    if dst.exists() {
        return Err(std::io::Error::other("conflict: dest exists"));
    }
    let mut files = 0u32;
    let mut total = 0u64;
    let mut stack = vec![src.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            let md = std::fs::symlink_metadata(&p)?;
            if md.file_type().is_symlink() {
                continue; // 链接不复制——断链比偷渡安全
            }
            if md.is_dir() {
                stack.push(p);
                continue;
            }
            files += 1;
            total += md.len();
            if files > 200 || total > 8 * 1024 * 1024 {
                return Err(std::io::Error::other("skill package too large"));
            }
            let rel = p.strip_prefix(src).map_err(std::io::Error::other)?;
            let to = dst.join(rel);
            std::fs::create_dir_all(to.parent().unwrap())?;
            std::fs::copy(&p, &to)?;
        }
    }
    Ok(files)
}

/// 批量导入外部技能目录 → 全局技能库。单条失败记入 skipped 不炸整批。
pub fn import_skills_at(dest_dir: &Path, paths: &[String]) -> ImportReport {
    let mut rep = ImportReport {
        imported: 0,
        skipped: vec![],
    };
    for p in paths {
        let src = Path::new(p);
        let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
            rep.skipped.push(format!("{p}: bad name"));
            continue;
        };
        if validate_name(name).is_err() {
            rep.skipped.push(format!("{name}: invalid name"));
            continue;
        }
        match copy_skill_dir(src, &dest_dir.join(name)) {
            Ok(_) => rep.imported += 1,
            Err(e) => rep.skipped.push(format!("{name}: {e}")),
        }
    }
    rep
}

pub fn import_skills(paths: &[String]) -> Result<ImportReport, std::io::Error> {
    let dest = global_dir().ok_or_else(|| std::io::Error::other("no HOME"))?;
    std::fs::create_dir_all(&dest)?;
    Ok(import_skills_at(&dest, paths))
}

/// 从路径安装技能包：文件夹 = 复制；ZIP = 解包（找含 SKILL.md 的顶层目录，
/// 防 zip-slip 逃逸走成员名校验）。同为导入语义：同名冲突拒绝。
pub fn install_skill_from_path_at(dest_root: &Path, path: &str) -> Result<String, std::io::Error> {
    let src = Path::new(path);
    std::fs::create_dir_all(dest_root)?;
    if src.is_dir() {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| std::io::Error::other("bad dir name"))?;
        validate_name(name).map_err(std::io::Error::other)?;
        if !src.join("SKILL.md").is_file() {
            return Err(std::io::Error::other("not a skill dir (no SKILL.md)"));
        }
        copy_skill_dir(src, &dest_root.join(name))?;
        return Ok(name.to_string());
    }
    // ZIP：先定位含 SKILL.md 的包根前缀（支持 zip 顶层单目录包裹的常见形态）
    let f = std::fs::File::open(src)?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut root_prefix: Option<String> = None;
    for i in 0..z.len() {
        let Ok(e) = z.by_index(i) else { continue };
        let Some(name) = e.enclosed_name() else {
            continue;
        }; // 拒 ../ 逃逸
        let s = name.to_string_lossy().replace('\\', "/");
        if s.ends_with("SKILL.md") {
            root_prefix = Some(s.strip_suffix("SKILL.md").unwrap_or("").to_string());
            break;
        }
    }
    let prefix = root_prefix.ok_or_else(|| std::io::Error::other("zip has no SKILL.md"))?;
    let skill_name = Path::new(prefix.trim_end_matches('/'))
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(|| {
            src.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("skill")
                .to_string()
        });
    validate_name(&skill_name).map_err(std::io::Error::other)?;
    let dst = dest_root.join(&skill_name);
    if dst.exists() {
        return Err(std::io::Error::other("conflict: dest exists"));
    }
    let mut files = 0u32;
    let mut total = 0u64;
    for i in 0..z.len() {
        let Ok(mut e) = z.by_index(i) else { continue };
        let Some(name) = e.enclosed_name() else {
            continue;
        };
        let s = name.to_string_lossy().replace('\\', "/");
        let Some(rel) = s.strip_prefix(&prefix) else {
            continue;
        };
        if rel.is_empty() || e.is_dir() {
            continue;
        }
        files += 1;
        total += e.size();
        if files > 200 || total > 8 * 1024 * 1024 {
            return Err(std::io::Error::other("skill package too large"));
        }
        let to = dst.join(rel);
        std::fs::create_dir_all(to.parent().unwrap())?;
        std::io::copy(&mut e, &mut std::fs::File::create(to)?)?;
    }
    Ok(skill_name)
}

pub fn install_skill_from_path(path: &str) -> Result<String, std::io::Error> {
    let dest = global_dir().ok_or_else(|| std::io::Error::other("no HOME"))?;
    install_skill_from_path_at(&dest, path)
}
