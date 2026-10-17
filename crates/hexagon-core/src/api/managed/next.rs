//! review R2（2026-10-17，managed#report「未完成」行内动作）：报告里「仍在等」的每一行带一个可执行的下一步
//! （原型：「配置发布目标」「查看证据」，负责人点名「补钥匙」）。reconciliation 16B 新偏差 3 当初以
//! 「`ManagedRecord` 没有可执行目标，不造入口」为由统一画「在时间线中查看」——这里由宿主按封闭表给出
//! 动作种类 + 目标 id，UI 只把种类映射到已有入口，不从理由文本猜。
//!
//! 代价模型：给错入口（比如该补钥匙却指去时间线）花负责人一次多点；指向一个不存在的入口则是死按钮。
//! 所以只认宿主写下的原因码与卡的当前状态，认不出的一律落 `Timeline`（永远可达的兜底）。
use super::*;

/// 下一步动作的种类（封闭集）。`target` 的含义随种类：设置类 = 设置分区 id，`OpenCard` = 卡 id，
/// `Evidence` / `Timeline` = 该条决定的轨迹事件 id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ManagedNextKind {
    /// 模型钥匙缺失 / 失效 / 读不出：设置 · 模型服务补钥匙。
    AddKey,
    /// 没有可用模型或配置被拒：设置 · 模型服务。
    ModelServices,
    /// 预算或额度用完：设置 · 用量。
    Usage,
    /// MCP 需要登录凭据或未安装：设置 · MCP 服务。
    Mcp,
    /// 技能未安装：设置 · 技能。
    Skills,
    /// 卡仍在等负责人（托管关闭后回到待决坞）：定位到这张卡。
    OpenCard,
    /// 验收没走完：看这条决定的证据（时间线定位）。
    Evidence,
    /// 兜底：在时间线中查看。
    Timeline,
}

/// 报告行的下一步（D06 DTO）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ManagedNext {
    pub kind: ManagedNextKind,
    pub target: String,
}

/// 下一步需要的宿主事实（读报告时取，不进事件载荷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NextFacts<'a> {
    pub event_id: i64,
    pub card_id: &'a str,
    pub card_kind: &'a str,
    pub reason: &'a str,
    pub result: &'a str,
    /// 卡此刻仍是 queued（失败后留在待决里，或托管关闭后回到负责人）。
    pub card_queued: bool,
    /// 授权卡的 `grant_kind`（mcp | skill）；其余卡为空。
    pub grant_kind: &'a str,
}

/// 与 `ModelRecoveryStatus` 的 SETTINGS_CODES / KEY_CODES 同口径（model_failure::failure_code 的码）。
const KEY_CODES: &[&str] = &["missing_credential", "credential_store", "authentication"];
const MODEL_CODES: &[&str] = &["no_model", "configuration", "request_rejected"];
const USAGE_CODES: &[&str] = &["budget_unavailable", "quota"];

/// 封闭表。已完成项只回时间线；卡还在等负责人时定位卡优先于任何设置入口（先处理眼前那张卡）。
pub(crate) fn next_step(facts: &NextFacts) -> ManagedNext {
    let event = || facts.event_id.to_string();
    let settings = |kind, section: &str| ManagedNext {
        kind,
        target: section.into(),
    };
    if facts.result == "done" {
        return ManagedNext {
            kind: ManagedNextKind::Timeline,
            target: event(),
        };
    }
    if facts.card_queued && !facts.card_id.is_empty() {
        return ManagedNext {
            kind: ManagedNextKind::OpenCard,
            target: facts.card_id.into(),
        };
    }
    if let Some(code) = facts.reason.strip_prefix("managed_route_failed:") {
        if KEY_CODES.contains(&code) {
            return settings(ManagedNextKind::AddKey, "models");
        }
        if MODEL_CODES.contains(&code) {
            return settings(ManagedNextKind::ModelServices, "models");
        }
        if USAGE_CODES.contains(&code) {
            return settings(ManagedNextKind::Usage, "usage");
        }
    }
    match (facts.card_kind, facts.reason, facts.grant_kind) {
        (_, "credentials_or_login_required", _) | (_, "capability_not_installed", "mcp") => {
            settings(ManagedNextKind::Mcp, "mcp")
        }
        (_, "capability_not_installed", "skill") => settings(ManagedNextKind::Skills, "skills"),
        ("stamp", _, _) => ManagedNext {
            kind: ManagedNextKind::Evidence,
            target: event(),
        },
        _ => ManagedNext {
            kind: ManagedNextKind::Timeline,
            target: event(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn facts<'a>(reason: &'a str, result: &'a str) -> NextFacts<'a> {
        NextFacts {
            event_id: 7,
            card_id: "q1",
            card_kind: "permission",
            reason,
            result,
            card_queued: false,
            grant_kind: "",
        }
    }

    #[test]
    fn host_codes_pick_the_existing_entry() {
        let kind = |f: NextFacts| next_step(&f).kind;
        assert_eq!(
            kind(facts("managed_route_failed:authentication", "incomplete")),
            ManagedNextKind::AddKey
        );
        assert_eq!(
            kind(facts("managed_route_failed:no_model", "incomplete")),
            ManagedNextKind::ModelServices
        );
        assert_eq!(
            kind(facts("managed_route_failed:quota", "failed")),
            ManagedNextKind::Usage
        );
        assert_eq!(
            kind(facts("credentials_or_login_required", "incomplete")),
            ManagedNextKind::Mcp
        );
        assert_eq!(
            kind(NextFacts {
                grant_kind: "skill",
                ..facts("capability_not_installed", "incomplete")
            }),
            ManagedNextKind::Skills
        );
        assert_eq!(
            kind(NextFacts {
                card_kind: "stamp",
                ..facts("no_active_stage", "incomplete")
            }),
            ManagedNextKind::Evidence
        );
        // 发布缺目标：应用里还没有配置发布目标的入口，不造死按钮。
        assert_eq!(
            next_step(&NextFacts {
                card_kind: "publish",
                ..facts("target_missing", "incomplete")
            }),
            ManagedNext {
                kind: ManagedNextKind::Timeline,
                target: "7".into()
            }
        );
        assert_eq!(
            next_step(&NextFacts {
                card_queued: true,
                ..facts("managed_route_failed:authentication", "failed")
            }),
            ManagedNext {
                kind: ManagedNextKind::OpenCard,
                target: "q1".into()
            }
        );
    }

    proptest! {
        /// D12：目标永远可达——设置类只指向已有分区，卡 / 事件类指向这条记录自己的 id；完成项只回时间线。
        #[test]
        fn every_next_step_points_at_a_reachable_entry(
            reason in prop::sample::select(vec!["managed_route_failed:authentication","managed_route_failed:no_model",
                "managed_route_failed:quota","managed_route_failed:transport","credentials_or_login_required",
                "capability_not_installed","target_missing","no_active_stage","x"]),
            result in prop::sample::select(vec!["done","incomplete","failed"]),
            card_kind in prop::sample::select(vec!["permission","stamp","publish","grant","install"]),
            grant_kind in prop::sample::select(vec!["","mcp","skill"]),
            card_queued in any::<bool>(),
            event_id in 1i64..1_000_000,
        ) {
            let f = NextFacts { event_id, card_id: "q1", card_kind, reason, result, card_queued, grant_kind };
            let next = next_step(&f);
            match next.kind {
                ManagedNextKind::AddKey | ManagedNextKind::ModelServices => prop_assert_eq!(next.target.as_str(), "models"),
                ManagedNextKind::Usage => prop_assert_eq!(next.target.as_str(), "usage"),
                ManagedNextKind::Mcp => prop_assert_eq!(next.target.as_str(), "mcp"),
                ManagedNextKind::Skills => prop_assert_eq!(next.target.as_str(), "skills"),
                ManagedNextKind::OpenCard => prop_assert!(card_queued && result != "done" && next.target == "q1"),
                ManagedNextKind::Evidence | ManagedNextKind::Timeline => prop_assert_eq!(next.target.clone(), event_id.to_string()),
            }
            if result == "done" {
                prop_assert_eq!(next.kind, ManagedNextKind::Timeline);
            }
        }
    }
}
