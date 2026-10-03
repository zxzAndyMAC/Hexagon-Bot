//! 设置「提示词」页（prompt-engineering 票 11 / ADR 0071）。
//!
//! 面向模型的固定提示词统一英文；负责人要理解工作台对 agent 说了什么时，
//! 这一页给出英文原文和当前界面语言的参考译文。目录只收代码里写死的文本，
//! 模板里的变量用 `{…}` 占位渲染——和运行时是同一个函数产出的同一份文本。
//! 负责人写的内容（项目说明、角色职责、技能正文）和 MCP 工具描述不进目录
//! （spec Q20/Q23）。
//!
//! 译文由宿主级 `translate` 模型槽实时生成（未绑定经 `resolve_slot` 落
//! default），按「语言 + 段落 id + 原文哈希」缓存：英文一改哈希就变，那段
//! 自动重翻；不改就零重复成本。被否决：译文随代码手工维护在七语言文件里
//! ——改英文不改译文就过期，负责人看到的「对照」是错的。
//!
//! 记账豁免：翻译是宿主调用，不落请求信封、不计任何项目的用量与上限。
//! 「派发路径 100% 落信封」（rsi-research 票 03）按项目轨迹界定；这里没有
//! 项目轨迹可挂，送出去的也只是固定工作台文本，不含项目数据（spec 裁决）。
//! 成功记宿主 Debug 诊断，失败记 Warn。

use crate::provider::{ChatRequest, ContentBlock, Message, ModelProvider, Role};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// 目录里的一段固定提示词。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PromptEntry {
    pub id: String,
    /// workbench / subagent / runtime / tools / judges / wizard / translator
    pub group: String,
    pub text: String,
    /// 原文 fnv64（16 位十六进制）——译文缓存键的一部分。
    pub hash: String,
}

/// 一段的参考译文；`text` 与 `error` 恰有一个。`error` 是原因码
/// （`not_json` / `missing` / `provider`），界面按码取 i18n 文案。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PromptTranslation {
    pub id: String,
    pub text: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TranslateOutcome {
    pub entries: Vec<PromptTranslation>,
    /// 翻译槽与 default 槽都没有可用模型：界面整页只显示英文并提示去配置。
    pub no_model: bool,
}

/// 翻译用的系统提示词（本身也进目录，分组 translator）。
pub const TRANSLATE_PROMPT: &str = "You translate reference documentation for a software workbench so its owner can understand the fixed instructions given to AI agents. The user message is a JSON object whose values are English texts. Translate every value into {language}.
- Keep these unchanged: tool names, parameter names, code, file paths, identifiers, JSON keys, command lines, and placeholders in curly braces such as {language}.
- Keep the markdown structure, line breaks and list markers.
- Translate faithfully; do not add, drop or soften any rule.
Output only a JSON object with exactly the same keys, and nothing else.";

fn entry(id: &str, group: &str, text: String) -> PromptEntry {
    PromptEntry {
        id: id.into(),
        group: group.into(),
        hash: format!("{:016x}", crate::tools::fnv64(&text)),
        text,
    }
}

/// 固定提示词目录。
pub fn catalog() -> Vec<PromptEntry> {
    use crate::turn::prompt as tp;
    let mut v = vec![
        entry("workbench.base", "workbench", tp::WORKBENCH_BASE.into()),
        entry(
            "workbench.reply_language",
            "workbench",
            tp::reply_language_section("{language}"),
        ),
        entry(
            "workbench.role_layer",
            "workbench",
            tp::role_layer_text(
                "{role}",
                "{duty written by the owner}",
                &["{owned path}".into()],
                &["{skill}".into()],
                Some(("{stage}", &["{deliverable kind}".into()])),
            ),
        ),
        entry(
            "subagent.base",
            "subagent",
            tp::WORKBENCH_BASE_SUBAGENT.into(),
        ),
        entry(
            "runtime.truncation",
            "runtime",
            crate::turn::TRUNCATION_NUDGE.into(),
        ),
        entry(
            "runtime.recovery",
            "runtime",
            crate::turn::RECOVERY_NUDGE.into(),
        ),
        entry(
            "runtime.denied",
            "runtime",
            crate::turn::DENIED_GUIDANCE.into(),
        ),
        entry(
            "runtime.plan_first",
            "runtime",
            crate::turn::PLAN_FIRST_INSTRUCTION.into(),
        ),
        entry(
            "runtime.task_reminder",
            "runtime",
            crate::turn::task_reminder_text("- [open] {task} ({task id})"),
        ),
        entry(
            "runtime.context_compacted",
            "runtime",
            crate::turn::compaction_note_template(),
        ),
    ];
    let main = crate::tools::Registry::builtin();
    let mut tools = main.defs();
    for d in main.subagent_scope(&[]).defs() {
        if !tools.iter().any(|t| t.name == d.name) {
            tools.push(d);
        }
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    for t in tools {
        v.push(entry(&format!("tool.{}", t.name), "tools", t.description));
    }
    v.extend([
        entry(
            "judge.reviewer",
            "judges",
            crate::reviewer::INSTRUCTIONS.into(),
        ),
        entry(
            "judge.pm_route",
            "judges",
            crate::pm_route::choice_prompt(
                Some("{stage}"),
                &["{role}".into()],
                &["{role}".into()],
                "{speaker}",
                "{message}",
            ),
        ),
        entry(
            "judge.execute",
            "judges",
            crate::execute::judgment_state("{proposal id}", "{surface}", "{proposal body}"),
        ),
        entry("judge.replay", "judges", crate::judge::prompt_head()),
        entry(
            "wizard.agents_md",
            "wizard",
            crate::setup::AGENTS_MD_OPTIMIZE_PROMPT.into(),
        ),
        entry(
            "wizard.questions",
            "wizard",
            crate::setup::BRIEF_QUESTIONS_PROMPT.into(),
        ),
        entry(
            "wizard.role_defs",
            "wizard",
            crate::setup::ROLE_SEEDS_PROMPT.into(),
        ),
        entry(
            "wizard.intake",
            "wizard",
            crate::intake::INTAKE_PROMPT.into(),
        ),
        entry(
            "wizard.flow",
            "wizard",
            crate::setup::flow_prompt("{project description}", &["{selected roles}".into()]),
        ),
        entry(
            "wizard.duty",
            "wizard",
            crate::setup::duty_prompt("{role}", "{hint}"),
        ),
        entry("translator", "translator", TRANSLATE_PROMPT.into()),
    ]);
    v
}

/// 失败原因码（界面 `settings.prompts_err_<code>`）。
const ERR_PROVIDER: &str = "provider";
const ERR_NOT_JSON: &str = "not_json";
const ERR_MISSING: &str = "missing";

type Cache = BTreeMap<String, String>;

fn cache_key(lang: &str, e: &PromptEntry) -> String {
    format!("{lang}|{}|{}", e.id, e.hash)
}

fn load_cache(path: &Path) -> Cache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cache(path: &Path, cache: &Cache) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(path, s);
    }
}

/// 一组一次请求：`{id: 英文}` → 模型只回 `{id: 译文}`。
fn translate_group(
    provider: &dyn ModelProvider,
    language: &str,
    group: &[&PromptEntry],
) -> Result<HashMap<String, String>, String> {
    let payload: BTreeMap<&str, &str> = group
        .iter()
        .map(|e| (e.id.as_str(), e.text.as_str()))
        .collect();
    let req = ChatRequest {
        model_slot: crate::provider_config::TRANSLATE_SLOT.into(),
        messages: vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: TRANSLATE_PROMPT.replace("{language}", language),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: serde_json::to_string(&payload).unwrap_or_default(),
                }],
            },
        ],
        tools: vec![],
    };
    let resp = provider
        .complete(&req)
        .map_err(|_| ERR_PROVIDER.to_string())?;
    let text = crate::intake::response_text(&resp);
    let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) else {
        return Err(ERR_NOT_JSON.into());
    };
    if end < start {
        return Err(ERR_NOT_JSON.into());
    }
    let v: Value =
        serde_json::from_str(&text[start..=end]).map_err(|_| ERR_NOT_JSON.to_string())?;
    let obj = v.as_object().ok_or_else(|| ERR_NOT_JSON.to_string())?;
    Ok(obj
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect())
}

/// 核心：按组翻译缺失（或 `force` 时全部）的段落并写缓存。英文界面不发请求。
pub fn translate_with(
    cache_path: &Path,
    entries: &[PromptEntry],
    lang: &str,
    provider: &dyn ModelProvider,
    force: bool,
) -> Vec<PromptTranslation> {
    if lang == "en" {
        return vec![];
    }
    let language = crate::uilang::language_name(Some(lang));
    let mut cache = load_cache(cache_path);
    let mut errors: HashMap<String, String> = HashMap::new();
    let mut groups: BTreeMap<&str, Vec<&PromptEntry>> = BTreeMap::new();
    for e in entries {
        if force || !cache.contains_key(&cache_key(lang, e)) {
            groups.entry(e.group.as_str()).or_default().push(e);
        }
    }
    for group in groups.values() {
        match translate_group(provider, language, group) {
            Ok(map) => {
                for e in group {
                    match map.get(&e.id) {
                        Some(t) => {
                            cache.insert(cache_key(lang, e), t.clone());
                        }
                        None => {
                            errors.insert(e.id.clone(), ERR_MISSING.into());
                        }
                    }
                }
            }
            Err(why) => {
                for e in group {
                    errors.insert(e.id.clone(), why.clone());
                }
            }
        }
    }
    save_cache(cache_path, &cache);
    entries
        .iter()
        .map(|e| {
            let error = errors.get(&e.id).cloned();
            PromptTranslation {
                id: e.id.clone(),
                text: if error.is_none() {
                    cache.get(&cache_key(lang, e)).cloned()
                } else {
                    None
                },
                error,
            }
        })
        .collect()
}

/// 宿主缓存文件（`HEXAGON_PROMPT_CACHE_PATH` 覆盖）。
fn cache_path() -> PathBuf {
    if let Ok(p) = std::env::var("HEXAGON_PROMPT_CACHE_PATH") {
        return PathBuf::from(p);
    }
    std::env::var_os("HOME")
        .map(|h| Path::new(&h).join(".hexagon/cache/prompt-translations.json"))
        .unwrap_or_else(|| PathBuf::from("/nonexistent/.hexagon/cache/prompt-translations.json"))
}

/// 宿主入口：解析翻译槽（未绑定落 default）→ 翻译 → 记诊断。
pub fn translate(lang: &str, force: bool) -> TranslateOutcome {
    // 不支持的码会被 language_name 回落成英文——白花钱「翻成英文」。
    if lang == "en" || !crate::uilang::SUPPORTED.contains(&lang) {
        return TranslateOutcome {
            entries: vec![],
            no_model: false,
        };
    }
    let started = std::time::Instant::now();
    let slot = crate::provider_config::TRANSLATE_SLOT;
    let fell_back = crate::provider_config::load()
        .map(|doc| crate::provider_config::fell_back_to_default(&doc.slots, slot))
        .unwrap_or(false);
    let provider =
        match crate::provider_admin::authoring_provider(crate::credentials::active(), slot) {
            Ok(p) => p,
            Err(_) => {
                crate::diag::note(
                    crate::diag::CLASS_SLOT,
                    true,
                    None,
                    None,
                    None,
                    None,
                    "prompt_translate",
                    "no_model",
                    started,
                );
                return TranslateOutcome {
                    entries: vec![],
                    no_model: true,
                };
            }
        };
    if fell_back {
        crate::diag::slot_fallback(None, None, None, "prompt_translate", slot, started);
    }
    let entries = translate_with(&cache_path(), &catalog(), lang, provider.as_ref(), force);
    let failed = entries.iter().filter(|e| e.error.is_some()).count();
    // 成功是宿主 Debug；失败是翻译槽上的模型调用失败 → 槽位 Warn。
    crate::diag::note(
        if failed > 0 {
            crate::diag::CLASS_SLOT
        } else {
            crate::diag::CLASS_HOST
        },
        failed > 0,
        None,
        None,
        None,
        None,
        "prompt_translate",
        &if failed > 0 {
            format!("failed:{failed}/{}", entries.len())
        } else {
            format!("ok:{}", entries.len())
        },
        started,
    );
    TranslateOutcome {
        entries,
        no_model: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;

    /// 回声桩：把每段英文换成「前缀 + 原文长度」，并记录请求。
    struct Echo {
        prefix: &'static str,
        seen: std::sync::Mutex<Vec<ChatRequest>>,
    }
    impl Echo {
        fn new(prefix: &'static str) -> Self {
            Self {
                prefix,
                seen: Default::default(),
            }
        }
        fn recorded(&self) -> Vec<ChatRequest> {
            self.seen.lock().unwrap().clone()
        }
    }
    impl ModelProvider for Echo {
        fn complete(
            &self,
            req: &ChatRequest,
        ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
            self.seen.lock().unwrap().push(req.clone());
            Ok(crate::turn::text_response(&echo_translation(
                req,
                self.prefix,
            )))
        }
    }

    fn echo_translation(req: &ChatRequest, prefix: &str) -> String {
        let ContentBlock::Text { text } = &req.messages[1].content[0] else {
            panic!()
        };
        let v: BTreeMap<String, String> = serde_json::from_str(text).unwrap();
        let out: BTreeMap<String, String> = v
            .into_iter()
            .map(|(k, t)| (k, format!("{prefix}{}", t.len())))
            .collect();
        serde_json::to_string(&out).unwrap()
    }

    fn sample() -> Vec<PromptEntry> {
        vec![
            entry("a.one", "a", "Alpha".into()),
            entry("a.two", "a", "Beta".into()),
            entry("b.one", "b", "Gamma".into()),
        ]
    }

    /// 目录收齐基础层、工具描述、判定类与翻译提示词本身；不收负责人内容。
    #[test]
    fn catalog_covers_fixed_prompts_with_stable_hashes() {
        let c = catalog();
        for id in [
            "workbench.base",
            "subagent.base",
            "tool.fs_patch",
            "tool.run_test",
            "judge.reviewer",
            "wizard.intake",
            "translator",
        ] {
            assert!(c.iter().any(|e| e.id == id), "目录缺 {id}");
        }
        let ids: std::collections::HashSet<_> = c.iter().map(|e| &e.id).collect();
        assert_eq!(ids.len(), c.len(), "id 不得重复");
        assert_eq!(catalog()[0].hash, c[0].hash, "同文同哈希");
    }

    #[test]
    fn english_interface_sends_nothing() {
        let d = tempfile::tempdir().unwrap();
        let p = ScriptedProvider::new(vec![]);
        assert!(translate_with(&d.path().join("c.json"), &sample(), "en", &p, false).is_empty());
        assert!(p.recorded().is_empty());
    }

    /// 一组一次请求；第二次打开全命中缓存零请求；原文一改只重翻那一组。
    #[test]
    fn cache_hits_then_hash_change_retranslates_only_that_group() {
        let d = tempfile::tempdir().unwrap();
        let cache = d.path().join("c.json");
        let first = Echo::new("ja:");
        let out = translate_with(&cache, &sample(), "ja", &first, false);
        assert_eq!(first.recorded().len(), 2, "两组两次请求");
        assert!(out.iter().all(|t| t.text.is_some() && t.error.is_none()));
        let sys = match &first.recorded()[0].messages[0].content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!(),
        };
        assert!(sys.contains("into Japanese"));

        let second = ScriptedProvider::new(vec![]);
        translate_with(&cache, &sample(), "ja", &second, false);
        assert!(second.recorded().is_empty(), "全命中缓存");

        let mut changed = sample();
        changed[2] = entry("b.one", "b", "Gamma changed".into());
        let third = Echo::new("ja:");
        let out = translate_with(&cache, &changed, "ja", &third, false);
        assert_eq!(third.recorded().len(), 1, "只重翻原文变了的那组");
        assert_eq!(out[2].text.as_deref(), Some("ja:13"));
    }

    #[test]
    fn bad_reply_marks_the_group_failed_and_keeps_others() {
        let d = tempfile::tempdir().unwrap();
        let p = ScriptedProvider::new(vec![
            crate::turn::text_response("sorry, I cannot"),
            crate::turn::text_response(r#"{"b.one":"ガンマ"}"#),
        ]);
        let out = translate_with(&d.path().join("c.json"), &sample(), "ja", &p, false);
        assert!(out[0].error.is_some() && out[0].text.is_none());
        assert!(out[1].error.is_some());
        assert_eq!(out[2].text.as_deref(), Some("ガンマ"));
    }

    #[test]
    fn force_retranslates_despite_cache() {
        let d = tempfile::tempdir().unwrap();
        let cache = d.path().join("c.json");
        let p = Echo::new("x");
        translate_with(&cache, &sample(), "fr", &p, false);
        translate_with(&cache, &sample(), "fr", &p, true);
        assert_eq!(p.recorded().len(), 4);
    }
}
