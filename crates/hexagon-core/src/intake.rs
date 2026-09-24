//! 现成仓库的只读开场分析（票 17 / ADR 0067）。
//!
//! 系统提示词就是 ADR 0067 后半（ADR 0071 起为英文版），一个字不改。命令不靠模型发挥：
//! 只承认仓库根清单里写明的构建/测试/检查，没有的位置写「未知」。
//! 模型若仍编出别的命令，从回复里拿掉那一行——编进时间线是一次
//! 假的构建方式（false command），删掉一句叙述花一次人工（false cut）。
//! 偏向删。
//!
//! 被否决的替代：给模型文件工具自己去读。工具能写、能远程发布，
//! 提示词挡不住。摘录是只读的，工具列表为空。
//!
//! 没有 AGENTS.md / CLAUDE.md 时，草案按优化描述的同一骨架另附。
//! 落盘不在这里。L4 也不在这里。

use crate::provider::{ChatResponse, ContentBlock};
use serde_json::Value;
use std::path::Path;

/// ADR 0067 开场分析的系统提示词（ADR 0071 修订：英文，回复用界面语言）。
/// 占位 `{language}`，由 [`intake_prompt`] 填。正文是合同，与 ADR 0067 逐字一致。
pub const INTAKE_PROMPT: &str = "You have just entered a repository that already contains files. Read only; do not change any file. Write an opening analysis based on the files that actually exist in the repository.

Put this in your reply:
- what the project is
- the build, test and check commands that can be determined from the files; for anything the files do not show, write \"{unknown}\" — never invent commands
- the entry points and the directory layout
- if AGENTS.md or CLAUDE.md already exists, only cite it; do not propose replacing it

If neither file exists, append an AGENTS.md draft after the analysis, using the same skeleton as the one-sentence optimisation, and mark it as a draft that is written to disk only after the owner confirms. Its commands, too, may only be ones the files establish.

Do not change business code, do not publish remotely, do not write secrets.

Write the reply in {language}.";

pub fn intake_prompt() -> String {
    INTAKE_PROMPT
        .replace("{language}", crate::uilang::reply_language())
        .replace("{unknown}", crate::owner_text::unknown())
}

/// 没有接话人时工作台自己写的那一句。作者不是花名册里的角色。
pub fn no_intake_speaker_note() -> &'static str {
    crate::owner_text::no_intake_speaker()
}

pub const STATUS_SKIP: &str = "skip";
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoCommands {
    pub build: Vec<String>,
    pub test: Vec<String>,
    pub check: Vec<String>,
}

impl RepoCommands {
    pub fn allowed(&self) -> Vec<String> {
        let mut out = Vec::new();
        for slot in [&self.build, &self.test, &self.check] {
            for cmd in slot {
                if !out.iter().any(|c| c == cmd) {
                    out.push(cmd.clone());
                }
            }
        }
        out
    }
}

/// 给负责人看的命令块（时间线里）。
pub fn commands_block(cmds: &RepoCommands) -> String {
    commands_block_with(
        cmds,
        crate::owner_text::unknown(),
        crate::owner_text::list_sep(),
    )
}

/// 空槽的占位词可换：发给模型的那份用英文（ADR 0071），时间线那份不动。
fn commands_block_with(cmds: &RepoCommands, unknown: &str, sep: &str) -> String {
    let line = |items: &[String]| {
        if items.is_empty() {
            unknown.to_string()
        } else {
            items.join(sep)
        }
    };
    format!(
        "## Commands\n- Build: {}\n- Test: {}\n- Check: {}\n",
        line(&cmds.build),
        line(&cmds.test),
        line(&cmds.check),
    )
}

fn push_unique(slot: &mut Vec<String>, cmd: String) {
    if !slot.iter().any(|c| c == &cmd) {
        slot.push(cmd);
    }
}

/// 只认仓库根的清单。子目录里的 package.json 可能是示例，当成项目命令是编造。
/// 漏掉 monorepo 子包的命令花一次人工补上；写错一条命令会进项目说明。偏向未知。
pub fn read_commands(root: &Path) -> RepoCommands {
    let mut cmds = RepoCommands::default();
    if let Some(text) = read_text(&root.join("package.json"), 256 * 1024) {
        merge_npm(&mut cmds, &text);
    }
    if root.join("Cargo.toml").is_file() {
        push_unique(&mut cmds.build, "cargo build".into());
        push_unique(&mut cmds.test, "cargo test".into());
    }
    for name in ["Makefile", "makefile", "GNUmakefile"] {
        if let Some(text) = read_text(&root.join(name), 256 * 1024) {
            merge_make(&mut cmds, &text);
            break;
        }
    }
    if root.join("go.mod").is_file() {
        push_unique(&mut cmds.build, "go build ./...".into());
        push_unique(&mut cmds.test, "go test ./...".into());
    }
    if let Some(text) = read_text(&root.join("pyproject.toml"), 256 * 1024) {
        if text.contains("[tool.pytest") {
            push_unique(&mut cmds.test, "pytest".into());
        }
    }
    cmds
}

fn merge_npm(cmds: &mut RepoCommands, text: &str) {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(scripts) = value.get("scripts").and_then(|v| v.as_object()) else {
        return;
    };
    for (key, slot) in [
        ("build", &mut cmds.build),
        ("test", &mut cmds.test),
        ("check", &mut cmds.check),
    ] {
        if let Some(body) = scripts.get(key).and_then(|v| v.as_str()) {
            if !body.trim().is_empty() {
                push_unique(slot, format!("npm run {key}"));
            }
        }
    }
}

fn merge_make(cmds: &mut RepoCommands, text: &str) {
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('.') {
            continue;
        }
        let Some((name, _)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        let slot = match name {
            "build" => &mut cmds.build,
            "test" => &mut cmds.test,
            "check" => &mut cmds.check,
            _ => continue,
        };
        push_unique(slot, format!("make {name}"));
    }
}

pub fn instruction_file(root: &Path) -> Option<&'static str> {
    if root.join("AGENTS.md").is_file() {
        Some("AGENTS.md")
    } else if root.join("CLAUDE.md").is_file() {
        Some("CLAUDE.md")
    } else {
        None
    }
}

pub fn layout_entries(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return names;
    };
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == ".hexagon" || name == ".DS_Store" {
            continue;
        }
        names.push(name);
    }
    names.sort();
    names.truncate(200);
    names
}

pub fn response_text(resp: &ChatResponse) -> String {
    resp.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// 模型回复里，不在 `allowed` 里的命令行整行去掉。
pub fn scrub_model_text(text: &str, allowed: &[String]) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if line_has_unallowed(line, allowed) {
            continue;
        }
        out.push_str(line);
    }
    out
}

pub fn about_line(scrubbed: &str) -> String {
    for line in scrubbed.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut s: String = t.chars().take(160).collect();
        if t.chars().count() > 160 {
            s.push('…');
        }
        return s;
    }
    crate::owner_text::unknown().into()
}

pub fn draft_body(name: &str, about: &str, cmds: &RepoCommands, layout: &[String]) -> String {
    let name = name.replace(['\n', '\r'], " ");
    let name = name.trim();
    let unknown = crate::owner_text::unknown();
    let name = if name.is_empty() { unknown } else { name };
    let about = about.trim();
    let about = if about.is_empty() { unknown } else { about };
    let mut layout_s = String::new();
    if layout.is_empty() {
        layout_s.push_str(&format!("- {unknown}\n"));
    } else {
        for n in layout {
            layout_s.push_str(&format!("- {n}\n"));
        }
    }
    format!(
        "# {name}\n\n## Purpose\n{about}\n\n{}\n## Layout\n{layout_s}\n## Conventions\n-\n",
        commands_block(cmds)
    )
}

/// `scrubbed` 已经去掉编造的命令。已有说明文件时不附草案。
pub fn compose_timeline(
    scrubbed: &str,
    cmds: &RepoCommands,
    cited: Option<&str>,
    draft: Option<&str>,
) -> String {
    let mut body = scrubbed.trim().to_string();
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(&commands_block(cmds));
    if let Some(file) = cited {
        body.push_str(&crate::owner_text::existing_instructions(file));
    } else if let Some(draft) = draft {
        body.push_str(crate::owner_text::draft_banner());
        body.push_str(draft);
        if !draft.ends_with('\n') {
            body.push('\n');
        }
    }
    body
}

pub fn user_prompt(root: &Path, project_name: &str, cmds: &RepoCommands) -> String {
    let mut out = String::new();
    out.push_str(&format!("Project name: {project_name}\n\n"));
    out.push_str("Repository root entries:\n");
    let entries = layout_entries(root);
    if entries.is_empty() {
        out.push_str("- (no visible entries)\n");
    } else {
        for name in &entries {
            let path = root.join(name);
            if path.is_dir() {
                out.push_str(&format!("- {name}/\n"));
            } else {
                out.push_str(&format!("- {name}\n"));
            }
        }
    }
    out.push_str(
        "\nCommands established from the files. Anything not listed is unknown — do not add or rewrite commands:\n",
    );
    out.push_str(&commands_block_with(cmds, "unknown", "; "));
    out.push_str("\nFile excerpts (read-only):\n");
    let mut budget = 16_000usize;
    for name in [
        "package.json",
        "Cargo.toml",
        "Makefile",
        "makefile",
        "GNUmakefile",
        "go.mod",
        "pyproject.toml",
        "README.md",
        "README",
        "AGENTS.md",
        "CLAUDE.md",
    ] {
        if is_secret_name(name) {
            continue;
        }
        let Some(text) = read_text(&root.join(name), 4000) else {
            continue;
        };
        if budget == 0 {
            break;
        }
        let take = text.chars().take(budget).collect::<String>();
        budget = budget.saturating_sub(take.chars().count());
        out.push_str(&format!("\n### {name}\n{take}\n"));
    }
    match instruction_file(root) {
        Some(file) => {
            out.push_str(&format!(
                "\nExisting project instructions: {file}. Only cite them; do not propose replacing them or writing another copy.\n"
            ));
        }
        None => {
            out.push_str("\nThere is no AGENTS.md and no CLAUDE.md.\n");
        }
    }
    out.push_str("Do not write any files, do not publish remotely, do not write secrets.\n");
    out
}

fn is_secret_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == ".env"
        || n.starts_with(".env.")
        || n.ends_with(".pem")
        || n == "id_rsa"
        || n == "credentials.json"
        || n.contains("secret")
}

fn read_text(path: &Path, cap_chars: usize) -> Option<String> {
    if is_secret_name(path.file_name()?.to_str()?) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut s: String = text.chars().take(cap_chars).collect();
    if text.chars().count() > cap_chars {
        s.push_str("\n…");
    }
    Some(s)
}

/// 命令词。长的在前，避免 `pnpm` 先吃掉 `pnpm run`。
const MARKERS: &[&str] = &[
    "npm run ",
    "npm test",
    "pnpm run ",
    "pnpm ",
    "yarn run ",
    "yarn ",
    "bun run ",
    "cargo clippy",
    "cargo build",
    "cargo test",
    "cargo check",
    "cargo fmt",
    "cargo run",
    "make ",
    "pytest",
    "go build",
    "go test",
    "mvn ",
    "gradle ",
];

fn line_has_unallowed(line: &str, allowed: &[String]) -> bool {
    let mut i = 0;
    while i < line.len() {
        if !boundary_before(line, i) {
            i += utf8_len_at(line, i);
            continue;
        }
        let rest = &line[i..];
        if let Some(marker) = MARKERS.iter().copied().find(|m| rest.starts_with(m)) {
            if marker_is_word(rest, marker) {
                let inv = invocation_at(rest, marker);
                if !allowed.iter().any(|a| a == &inv) {
                    return true;
                }
            }
            i += marker.len();
            continue;
        }
        i += utf8_len_at(line, i);
    }
    false
}

fn boundary_before(line: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    !line[..i]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn marker_is_word(rest: &str, marker: &str) -> bool {
    if marker.ends_with(' ') {
        return true;
    }
    match rest[marker.len()..].chars().next() {
        None => true,
        Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '.' => false,
        Some(_) => true,
    }
}

fn invocation_at(rest: &str, marker: &str) -> String {
    let after = &rest[marker.len()..];
    if marker.ends_with(' ') {
        let token = after
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| "，。,;；".contains(c));
        return format!("{marker}{token}");
    }
    if after.starts_with(' ') || after.starts_with('\t') {
        if let Some(token) = after.split_whitespace().next() {
            let token = token.trim_matches(|c: char| "，。,;；".contains(c));
            if token.starts_with('-') || token.starts_with('.') || token.contains('/') {
                return format!("{marker} {token}");
            }
        }
    }
    marker.to_string()
}

fn utf8_len_at(line: &str, i: usize) -> usize {
    line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ADR 0071 修订：合同原文换成英文版（ADR 0067 同步修订）。
    const ADR_0067_INTAKE_PROMPT: &str = "You have just entered a repository that already contains files. Read only; do not change any file. Write an opening analysis based on the files that actually exist in the repository.

Put this in your reply:
- what the project is
- the build, test and check commands that can be determined from the files; for anything the files do not show, write \"{unknown}\" — never invent commands
- the entry points and the directory layout
- if AGENTS.md or CLAUDE.md already exists, only cite it; do not propose replacing it

If neither file exists, append an AGENTS.md draft after the analysis, using the same skeleton as the one-sentence optimisation, and mark it as a draft that is written to disk only after the owner confirms. Its commands, too, may only be ones the files establish.

Do not change business code, do not publish remotely, do not write secrets.

Write the reply in {language}.";

    #[test]
    fn prompt_is_the_adr_text() {
        assert_eq!(INTAKE_PROMPT, ADR_0067_INTAKE_PROMPT);
    }

    #[test]
    fn package_json_only_named_scripts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"test":"vitest","lint":"oxlint"}}"#,
        )
        .unwrap();
        let cmds = read_commands(dir.path());
        assert!(cmds.build.is_empty());
        assert_eq!(cmds.test, vec!["npm run test".to_string()]);
        assert!(cmds.check.is_empty());
        let block = commands_block(&cmds);
        assert!(block.contains("- Build: unknown"));
        assert!(block.contains("- Test: npm run test"));
        assert!(block.contains("- Check: unknown"));
        assert!(!block.contains("oxlint"));
        assert!(!block.contains("npm run lint"));
        assert!(!block.contains("npm run build"));
    }

    #[test]
    fn empty_script_body_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"  ","test":"vitest"}}"#,
        )
        .unwrap();
        let cmds = read_commands(dir.path());
        assert!(cmds.build.is_empty());
        assert_eq!(cmds.test, vec!["npm run test".to_string()]);
    }

    #[test]
    fn cargo_make_and_go_only_when_the_file_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(
            dir.path().join("Makefile"),
            "build:\n\ttrue\ncheck:\n\ttrue\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/x\n").unwrap();
        let cmds = read_commands(dir.path());
        assert_eq!(
            cmds.build,
            vec![
                "cargo build".to_string(),
                "make build".to_string(),
                "go build ./...".to_string()
            ]
        );
        assert_eq!(
            cmds.test,
            vec!["cargo test".to_string(), "go test ./...".to_string()]
        );
        assert_eq!(cmds.check, vec!["make check".to_string()]);
        assert!(!commands_block(&cmds).contains("cargo clippy"));
    }

    #[test]
    fn nested_manifest_is_not_a_project_command() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("examples")).unwrap();
        std::fs::write(
            dir.path().join("examples/package.json"),
            r#"{"scripts":{"build":"tsc"}}"#,
        )
        .unwrap();
        let cmds = read_commands(dir.path());
        assert!(cmds.build.is_empty());
        assert!(!commands_block(&cmds).contains("npm"));
    }

    #[test]
    fn scrub_drops_invented_commands_and_keeps_the_sentence() {
        let allowed = vec!["npm run test".to_string()];
        let got = scrub_model_text(
            "这是一个前端小项目。\n构建命令是 npm run build。\n测试用 npm run test。\n",
            &allowed,
        );
        assert!(got.contains("前端小项目"));
        assert!(got.contains("npm run test"));
        assert!(!got.contains("npm run build"));
    }

    #[test]
    fn secret_files_stay_out_of_the_prompt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "SUPERSECRETKEY=abc\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        let prompt = user_prompt(dir.path(), "Demo", &RepoCommands::default());
        assert!(!prompt.contains("SUPERSECRETKEY"));
        assert!(prompt.contains("hello"));
        // prompt-engineering 票 09：user 消息英文化（ADR 0071）。
        assert!(prompt.contains("There is no AGENTS.md"));
        assert!(user_prompt(dir.path(), "Demo", &read_commands(dir.path())).contains("unknown"));
    }

    fn script_body() -> impl Strategy<Value = Option<String>> {
        prop_oneof![
            Just(None),
            Just(Some(String::new())),
            "[a-z]{1,8}".prop_map(Some),
        ]
    }

    proptest! {
        /// 不变量：渲染出来的 npm 命令只可能是文件里非空的 build/test/check。
        /// 其它脚本名、空字符串、cargo，都不得出现。
        #[test]
        fn npm_commands_never_leave_the_three_keys(
            build in script_body(),
            test in script_body(),
            check in script_body(),
            extra in "[a-z]{3,8}",
        ) {
            prop_assume!(extra != "build" && extra != "test" && extra != "check");
            let dir = tempfile::tempdir().unwrap();
            let mut scripts = serde_json::Map::new();
            for (key, body) in [("build", &build), ("test", &test), ("check", &check)] {
                if let Some(body) = body {
                    scripts.insert((*key).into(), Value::String(body.clone()));
                }
            }
            scripts.insert(extra.clone(), Value::String("echo hi".into()));
            let pkg = serde_json::json!({ "scripts": scripts });
            std::fs::write(dir.path().join("package.json"), pkg.to_string()).unwrap();
            let cmds = read_commands(dir.path());
            let block = commands_block(&cmds);
            let invented = format!("npm run {extra}");
            prop_assert!(!block.contains(&invented));
            prop_assert!(!block.contains("cargo"));
            prop_assert!(!block.contains("oxlint"));
            let expect = |body: &Option<String>, slot: &[String], key: &str| {
                let present = body.as_ref().is_some_and(|s| !s.is_empty());
                let cmd = format!("npm run {key}");
                if present {
                    slot.iter().any(|c| c == &cmd)
                } else {
                    !slot.iter().any(|c| c.contains("npm"))
                }
            };
            prop_assert!(expect(&build, &cmds.build, "build"));
            prop_assert!(expect(&test, &cmds.test, "test"));
            prop_assert!(expect(&check, &cmds.check, "check"));
        }
    }
}
