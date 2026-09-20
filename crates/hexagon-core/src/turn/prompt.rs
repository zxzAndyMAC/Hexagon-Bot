//! 提示词装配与请求信封（turn 内核的「喂给模型什么」侧）。
//!
//! 优先级链（高→低）：工作台约束 > 流程包 > AGENTS.md > 角色定义 > 激活简报。
//! 同 key 的层冲突时高层胜出、低层整段丢弃；无 key 层全部保留。
//! 请求信封（request_envelope/fingerprint）落「模型实际看到的请求」的指纹
//! 素材——装配归这里，信封也归这里（不变量伴随件 invariant.rs 按同一配方
//! 重算比对）。

use crate::db::Db;
use crate::provider::{ChatRequest, ContentBlock, Message, Role};
use crate::trace::MessageToken;
use crate::turn::TurnError;
use serde_json::{json, Value};
use std::path::Path;

/// 层级，数值越小优先级越高。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerLevel {
    Workbench = 0,
    Pack = 1,
    AgentsMd = 2,
    RoleDef = 3,
    Brief = 4,
}

impl LayerLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Workbench => "workbench",
            Self::Pack => "pack",
            Self::AgentsMd => "agents.md",
            Self::RoleDef => "role",
            Self::Brief => "brief",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptLayer {
    pub level: LayerLevel,
    /// 同 key 冲突时高层胜出；None = 不参与冲突判定的自由段落。
    pub key: Option<String>,
    pub text: String,
}

impl PromptLayer {
    pub fn new(level: LayerLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            key: None,
            text: text.into(),
        }
    }
    pub fn keyed(level: LayerLevel, key: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            level,
            key: Some(key.into()),
            text: text.into(),
        }
    }
}

/// 装配系统提示词：key 冲突只留最高层；输出按层级高→低排序、带段标。
pub fn build_system_prompt(layers: Vec<PromptLayer>) -> String {
    let mut per_key: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, l) in layers.iter().enumerate() {
        if let Some(k) = &l.key {
            let e = per_key.entry(k.clone()).or_insert(i);
            if layers[*e].level > l.level {
                *e = i;
            }
        }
    }
    let winning: std::collections::HashSet<usize> = per_key.values().copied().collect();
    let mut out: Vec<&PromptLayer> = layers
        .iter()
        .enumerate()
        .filter(|(i, l)| l.key.is_none() || winning.contains(i))
        .map(|(_, l)| l)
        .collect();
    out.sort_by_key(|l| l.level);
    out.iter()
        .map(|l| format!("## {}\n{}", l.level.label(), l.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 项目说明注入上限：超 ~32KB 降级「头部+节标题目录+必读指引」并提醒负责人（US72）。
const INSTRUCTIONS_CAP: usize = 32 * 1024;
/// 降级时保留的头部字节数。
const INSTRUCTIONS_HEAD: usize = 4 * 1024;

/// 动态尾部块（票 14，OPE `_trailing_block`）：易变内容（时间戳、轮次）
/// 独立成末尾 user 消息、发送时才拼——系统提示与历史前缀逐字节稳定，
/// provider prompt cache 才能命中。时间戳这类易变值别塞进系统层。
pub(super) fn with_dynamic_tail(messages: &[Message], round: usize) -> Vec<Message> {
    let mut out = messages.to_vec();
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    out.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: json!({"env": {"unix_time": secs, "round": round}}).to_string(),
        }],
    });
    out
}

/// 读项目说明（AGENTS.md 优先，CLAUDE.md 次）供激活注入。
/// 返回 (注入文本, 是否降级)；无说明文件返回 None。
pub fn load_instructions(repo_root: &Path) -> Option<(String, bool)> {
    let (name, full) = ["AGENTS.md", "CLAUDE.md"].iter().find_map(|n| {
        std::fs::read_to_string(repo_root.join(n))
            .ok()
            .map(|c| (*n, c))
    })?;
    if full.len() <= INSTRUCTIONS_CAP {
        return Some((format!("{name} 全文：\n{full}"), false));
    }
    // 降级：头部 + 节标题目录 + 必读指引（不做自动全量摘要——影响语义的决策不自动做）
    let mut end = INSTRUCTIONS_HEAD.min(full.len());
    while !full.is_char_boundary(end) {
        end -= 1;
    }
    let headings: Vec<&str> = full
        .lines()
        .filter(|l| l.starts_with('#'))
        .map(|l| l.trim())
        .collect();
    let text = format!(
        "{name}（全文 {} 字节 > 32KB，已降级）\n\
         必读指引：下面是文件头与节标题目录；需要某节细节时用 fs_read 读 {name} 对应位置。\n\
         --- 文件头 ---\n{}\n--- 节标题目录 ---\n{}",
        full.len(),
        &full[..end],
        headings.join("\n")
    );
    Some((text, true))
}

// ---------- 窄上下文 ----------

/// 激活简报上下文：只装指针，不装全文。
#[derive(Debug)]
pub struct BriefContext {
    /// 当前阶段已交付产物的指针（path + kind + version）
    pub artifacts: Vec<Value>,
    /// 上游交接说明（当前阶段产物的 upstream 链）
    pub upstream: Vec<Value>,
    /// 点名/回复该 Agent 的消息
    pub mentions: Vec<String>,
    /// `#` 路径指针：随点名消息携带的仓内路径，Agent 经工具层去读（非全文注入）
    pub paths: Vec<String>,
    /// 唤醒/打回通知
    pub notices: Vec<Value>,
    /// 事件高水位（票 10）：brief 组装时库里的最大事件 id。
    /// 首个 provider 响应到手后推进 agent_cursors 至此——推进早于送达就丢增量。
    pub watermark: i64,
}

pub fn build_brief_context(
    db: &Db,
    agent_id: &str,
    stage_run_id: Option<&str>,
) -> Result<BriefContext, TurnError> {
    let role: String =
        db.conn()
            .query_row("SELECT role FROM agents WHERE id = ?1", [agent_id], |r| {
                r.get(0)
            })?;
    let project_id: String = db.conn().query_row(
        "SELECT project_id FROM agents WHERE id = ?1",
        [agent_id],
        |r| r.get(0),
    )?;

    let artifacts = {
        let mut st = db.conn().prepare(
            "SELECT path, kind, version FROM artifacts
             WHERE project_id = ?1 AND status IN ('valid','stamped')
             AND (?2 IS NULL OR stage_run_id = ?2) ORDER BY created_at",
        )?;
        let rows = st.query_map(rusqlite::params![project_id, stage_run_id], |r| {
            Ok(json!({"path": r.get::<_,String>(0)?, "kind": r.get::<_,String>(1)?, "version": r.get::<_,i64>(2)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let upstream = {
        let mut st = db.conn().prepare(
            "SELECT a.path, a.kind, u.path FROM artifacts a
             JOIN artifacts u ON u.id = a.upstream_id
             WHERE a.project_id = ?1 AND (?2 IS NULL OR a.stage_run_id = ?2)",
        )?;
        let rows = st.query_map(rusqlite::params![project_id, stage_run_id], |r| {
            Ok(json!({"path": r.get::<_,String>(0)?, "kind": r.get::<_,String>(1)?, "upstream": r.get::<_,String>(2)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    // 票 10：per-agent 事件游标——brief 只增量读「上次读到位置之后」的事件，
    // 不再全表扫 mentions/notices。游标在首个 provider 响应到手后推进
    // （调用方负责 advance_cursor）——推进早了丢增量，晚了只是重送。
    let cursor = db.cursor(agent_id);
    let watermark: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1",
            [&project_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // 点名消息：tokens JSON 里含该角色 mention；走事件表（消息都有配对事件）
    // 才能用游标过滤——messages.id 与 events.id 不共享序列。
    let (mentions, paths) = {
        let mut st = db.conn().prepare(
            "SELECT m.body, m.tokens FROM events e
             JOIN messages m ON m.id = json_extract(e.payload,'$.message_id')
             WHERE e.project_id = ?1 AND e.id > ?2
             AND e.kind IN ('owner_message','agent_message')
             ORDER BY e.id",
        )?;
        let rows = st
            .query_map(rusqlite::params![project_id.clone(), cursor], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut paths = Vec::new();
        let mentions = rows
            .into_iter()
            .filter(|(_, tokens)| {
                let toks: Vec<MessageToken> = serde_json::from_str(tokens).unwrap_or_default();
                let named = toks.iter().any(
                    |t| matches!(t, MessageToken::Mention { agent_role } if agent_role == &role),
                );
                if named {
                    for t in toks {
                        if let MessageToken::PathRef { path } = t {
                            paths.push(path);
                        }
                    }
                }
                named
            })
            .map(|(body, _)| body)
            .collect();
        (mentions, paths)
    };

    // 唤醒/打回通知：指向该 Agent 的裁决/唤醒事件——同样按游标取增量。
    let notices = {
        let mut st = db.conn().prepare(
            "SELECT kind, payload FROM events
             WHERE project_id = ?1 AND agent_id = ?2 AND id > ?3
             AND kind IN ('flag_adjudicated','consult_wakeup','review_rejected')
             ORDER BY id",
        )?;
        let rows = st
            .query_map(rusqlite::params![project_id, agent_id, cursor], |r| {
                Ok(json!({"kind": r.get::<_,String>(0)?, "payload": r.get::<_,String>(1)?}))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    Ok(BriefContext {
        artifacts,
        upstream,
        mentions,
        paths,
        notices,
        watermark,
    })
}

// ---------- 请求信封 ----------

/// 请求信封（rsi-research 票 03）：每次模型派发落一条
/// System{kind:"request_envelope"} 事件——layer 来源清单（层级/key/
/// 内容 hash/字节数）+ 逐消息指纹 + 工具名单 + 模型参数 + 总指纹。
/// 信封只含 hash 不含正文：不变量伴随件（票 07）用同一构造函数从
/// trace 侧重建素材比对,指纹不符即 invariant_violation。
/// hash 用 fnv64 不用 DefaultHasher——后者种子随进程变,指纹要跨
/// 重启/回放稳定。
/// layer 元数据在 build_system_prompt 消费 layers 前预取——
/// 信封只存指纹素材,不存正文。
pub(super) fn layer_meta(layers: &[PromptLayer]) -> Vec<Value> {
    layers
        .iter()
        .map(|l| {
            json!({
                "level": l.level.label(),
                "key": l.key,
                "sha": format!("{:016x}", crate::tools::fnv64(&l.text)),
                "bytes": l.text.len(),
            })
        })
        .collect()
}

/// 信封载荷本体（见上注）：`layer_src` 为 layer_meta 的预取结果。
/// 指纹配方（票 03/07 共享）：信封落盘与不变量伴随件重算必须用同一
/// 序列化——改这里两边一起改,否则全是误报。
pub(crate) fn envelope_fingerprint(
    layers: &[Value],
    messages: &[Value],
    tools: &[&str],
    params: &Value,
) -> String {
    format!(
        "{:016x}",
        crate::tools::fnv64(
            &json!({
                "layers": layers, "messages": messages,
                "tools": tools, "params": params
            })
            .to_string()
        )
    )
}

/// `msgs` 是语义载荷（动态尾之前）：尾里的时间戳/轮次是运行时注记，
/// 不是输入语义——指纹盖它则同一请求每次都不一样,票 07 无从重建比对。
/// pub(crate)：judge.rs 的 LLM 判定派发也走同一信封（票 03「派发路径
/// 100% 落信封」——judge 调用也是模型派发，不许旁路）。
pub(crate) fn request_envelope(
    call: usize,
    req: &ChatRequest,
    semantic_msgs: &[Message],
    layer_src: &[Value],
) -> Value {
    let msg_refs: Vec<Value> = semantic_msgs
        .iter()
        .map(|m| {
            json!({
                "role": serde_json::to_value(&m.role).unwrap_or_default(),
                "sha": format!("{:016x}", crate::tools::fnv64(
                    &serde_json::to_string(&m.content).unwrap_or_default())),
            })
        })
        .collect();
    let tools: Vec<&str> = req.tools.iter().map(|t| t.name.as_str()).collect();
    let params = json!({"model_slot": req.model_slot});
    let fingerprint = envelope_fingerprint(layer_src, &msg_refs, &tools, &params);
    json!({
        "kind": "request_envelope",
        "call": call,
        "model_slot": req.model_slot,
        "layers": layer_src,
        "messages": msg_refs,
        "tools": tools,
        "params": params,
        "fingerprint": fingerprint,
    })
}
