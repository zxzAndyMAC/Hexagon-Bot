//! 项目经理的封闭选择（票 08 / ADR 0065）。
//!
//! 负责人没点名时，下一手不是聊天：模型只许吐花名册里的一个角色名，
//! 或「先不派活」。决策槽配了就用那个槽；没配则同一次选择走主对话槽。
//! 两种都不是聊天回合——聊天回复仍走主对话模型的正常回合。
//!
//! 代价模型：把花名册外的字符串当成派活 = 一次没人审过的回合
//! （false accept）；拒掉一次含糊输出 = 这句话暂时没人接，负责人还在
//! （false reject）。偏向拒绝。不抽子串、不剥 JSON：多一个字就不是
//! 封闭选择。出处：ADR 0065；被否决的替代是「模型句子里出现角色名就派」。

use crate::provider::{ChatRequest, ChatResponse, ContentBlock, Message, Role};

/// 预置角色名。向导默认勾选。卸掉之后的接话人在工作台门面（票 09）。
pub const PM_ROLE: &str = "项目经理";

/// 封闭选择里的「不唤醒任何人」。与角色名同级，不是自由文本。
/// 这是落库决策记录（eligible/chosen）与界面上的写法——回放基线按它比对，不改。
pub const HOLD: &str = "先不派活";

/// 发给模型的「不唤醒任何人」令牌（prompt-engineering 票 10 / ADR 0071）。
/// 提示词英文化后仍要模型回中文「先不派活」会诱发翻译式漂移（回 "Hold off"
/// 之类）而被判拒绝；给一个语言无关的令牌，解析后映射回 [`HOLD`]。
pub const HOLD_TOKEN: &str = "HOLD";

/// 时间线上工作台自己的说明。不是花名册里的角色，也不伪造角色发言。
pub const WORKBENCH_AUTHOR: &str = "工作台";

/// 没有接话人时写入时间线的那一句。作者是 [`WORKBENCH_AUTHOR`]。
pub const NO_RECEIVER_NOTE: &str =
    "没有接话的人。项目经理未勾选；流程包要当前阶段激活名单的第一位，快速通道要通道角色，这里都没有。";

/// 点名要不要唤醒被点名的角色。
///
/// ADR 0069 取消自治档之后，负责人的点名和角色的点名都派活。点名不看档位，
/// 也不经过「先不派活」。`from_owner` 留在签名里，是因为负责人和角色的
/// 点名以后仍可能分开记，今天两条都唤醒。
///
/// 被否决：留着秩参数但不读。调用方还会去读 `projects.autonomy`。
/// 出处：ADR 0061、ADR 0065、ADR 0069。
pub fn mention_wakes(from_owner: bool) -> bool {
    let _ = from_owner;
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteChoice {
    /// 花名册里的一个角色。调用方负责唤醒；本类型不保证它在激活名单里。
    Dispatch(String),
    Hold,
}

/// `raw` 去掉首尾空白后必须与花名册某一角色或 [`HOLD`] 逐字相等。
/// 花名册外、空、多出来的字 → `None`（拒绝，不派）。
pub fn parse_route_choice(raw: &str, roster: &[String]) -> Option<RouteChoice> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // HOLD 优先于花名册：没有角色该叫这个名字；即便有，也不派活。
    // 旧令牌仍收：两者都是封闭集合里的精确串，不放宽匹配。
    if s == HOLD_TOKEN || s == HOLD {
        return Some(RouteChoice::Hold);
    }
    if roster.iter().any(|r| r == s) {
        Some(RouteChoice::Dispatch(s.to_string()))
    } else {
        None
    }
}

/// 给 Jev 的状态。只描述局面，不要求模型自己吐一行字——选项走选择题接口。
/// 英文（ADR 0071）；阶段名、角色名、负责人原话逐字保留。
pub fn choice_state(
    stage: Option<&str>,
    activation: &[String],
    speaker: &str,
    body: &str,
) -> String {
    let stage_line = stage.unwrap_or("(no stage in progress)");
    let act = if activation.is_empty() {
        "(none)".to_string()
    } else {
        activation.join(", ")
    };
    format!(
        "Current stage: {stage_line}\nActivation list of this stage: {act}\n{speaker} just finished speaking without naming who goes next:\n{body}"
    )
}

/// 给决策调用的用户消息。选项逐行列出，模型被要求原样回一行。
/// 激活名单只是状态，不是选项边界——花名册里的人都可以被选。
/// `speaker` 是「the owner」或刚说完的角色名，不是自由发挥的句子。
pub fn choice_prompt(
    stage: Option<&str>,
    activation: &[String],
    roster: &[String],
    speaker: &str,
    body: &str,
) -> String {
    let stage_line = stage.unwrap_or("(no stage in progress)");
    let act = if activation.is_empty() {
        "(none)".to_string()
    } else {
        activation.join(", ")
    };
    let mut options = roster.to_vec();
    options.push(HOLD_TOKEN.to_string());
    format!(
        "You are the project manager. Make one closed choice: no chat, no explanation.\n\
         Current stage: {stage_line}\n\
         Activation list of this stage: {act}\n\
         You may pick roster roles outside the activation list. {HOLD_TOKEN} means wake no one for now.\n\
         {speaker} just finished speaking without naming who goes next:\n{body}\n\
         Output exactly one of the lines below, verbatim, with no punctuation or other text:\n{}",
        options.join("\n")
    )
}

/// 封闭选择的请求：无工具。带工具的是聊天回合，不是这道选择。
pub fn choice_request(slot: &str, prompt: &str) -> ChatRequest {
    ChatRequest {
        model_slot: slot.to_string(),
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
        }],
        tools: vec![],
    }
}

pub fn choice_text(resp: &ChatResponse) -> String {
    resp.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// prompt-engineering 票 10：模型看到的是语言无关令牌 HOLD，解析后等同
    /// 「先不派活」；旧中文令牌仍收。多一个字照旧拒绝。
    #[test]
    fn hold_token_is_language_neutral_and_exact() {
        let roster = vec!["后端".to_string()];
        assert_eq!(
            parse_route_choice(" HOLD\n", &roster),
            Some(RouteChoice::Hold)
        );
        assert_eq!(parse_route_choice(HOLD, &roster), Some(RouteChoice::Hold));
        assert!(parse_route_choice("Hold off", &roster).is_none());
        assert!(parse_route_choice("HOLD.", &roster).is_none());
        let p = choice_prompt(None, &[], &roster, "The owner", "x");
        assert!(p.lines().any(|l| l == HOLD_TOKEN) && p.lines().any(|l| l == "后端"));
    }

    #[test]
    fn exact_roster_or_hold_only() {
        let roster = vec!["后端".into(), "产品策划".into()];
        assert_eq!(
            parse_route_choice("后端", &roster),
            Some(RouteChoice::Dispatch("后端".into()))
        );
        assert_eq!(
            parse_route_choice("  先不派活\n", &roster),
            Some(RouteChoice::Hold)
        );
        // 花名册外的真角色名、多一个字、空，都拒绝——不许靠子串派活。
        assert!(parse_route_choice("架构师", &roster).is_none());
        assert!(parse_route_choice("后端 请开始", &roster).is_none());
        assert!(parse_route_choice("后端。", &roster).is_none());
        assert!(parse_route_choice("先不派", &roster).is_none());
        assert!(parse_route_choice("", &roster).is_none());
        assert!(parse_route_choice("   ", &roster).is_none());
    }

    proptest! {
        /// 不变量：派出去的名字一定在花名册里；其余输入一律不派。
        /// false accept 会唤醒花名册外的人，所以宁可不派。
        #[test]
        fn choice_outside_roster_never_dispatches(
            names in prop::collection::vec("[a-z]{1,12}", 0..8usize),
            raw in "\\PC{0,40}",
        ) {
            let mut roster = Vec::new();
            for n in names {
                if n != HOLD && !roster.contains(&n) {
                    roster.push(n);
                }
            }
            match parse_route_choice(&raw, &roster) {
                Some(RouteChoice::Dispatch(role)) => {
                    prop_assert!(roster.iter().any(|r| r == &role));
                    prop_assert_eq!(raw.trim(), role);
                }
                Some(RouteChoice::Hold) => prop_assert_eq!(raw.trim(), HOLD),
                None => {
                    let t = raw.trim();
                    prop_assert_ne!(t, HOLD);
                    prop_assert!(!roster.iter().any(|r| r == t));
                }
            }
        }
    }

    proptest! {
        /// 不变量：取消档位之后，负责人点名和角色点名都唤醒。
        /// 漏唤醒 = 点名留在群聊里，派活退回成只说话。
        #[test]
        fn mentions_always_wake(from_owner in any::<bool>()) {
            prop_assert!(mention_wakes(from_owner));
        }
    }
}
