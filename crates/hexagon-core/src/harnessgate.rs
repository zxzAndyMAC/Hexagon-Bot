//! 自治 L4 的 harness 放行判定（hands-free 票 04 / ADR 0063）。
//!
//! 只回答「这一类确认在这个存储档自动通过，还是等人」。不读库，不落盘。
//! 读的是存储档 `autonomy::rank`（0–4），不是 `execution_rank`。
//! 被否决：把 `execution_rank` 的 `min(2)` 抬到 4。封顶留着，是因为
//! false positive 的代价是未审的提案、授权或安装生效，而且会连坐
//! 仍在读执行档的打回路径。盖章和安全网已经各自读存储档（票 02/03）。
//!
//! 自动通过只限四件，而且只在存储档恰好是 4：改进提案的负责人盖章、
//! 技能授权确认、MCP 授权确认、自然语言安装确认。开场项目说明草案、
//! 远程发布、最终验收、内置永不任何档都不在此列。
//!
//! 授权和安装的写入面恒为当前项目。判定本身不带路径；调用方不得把
//! `AutoPass` 理解成可以写 `~/.hexagon`。
//!
//! 代价：L4 误判成等人，负责人多回来一次（false negative，可补确认）。
//! L0–L3 或禁区误判成自动通过，是一次未审副作用（false positive）。
//! 实现偏向等人：档不是恰好 4、动作不在四件白名单，一律等。

/// 一张确认在问的是哪件事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessAction {
    /// 改进提案的负责人盖章。上级复审通过之后才问这一关。
    ProposalOwnerStamp,
    /// 把一个技能写进当前项目的授权表。
    SkillGrant,
    /// 把一个 MCP 服务写进当前项目的授权表。
    McpGrant,
    /// 自然语言安装确认。落点是项目技能目录或项目 MCP 清单。
    NlInstall,
    /// 开场分析附的项目说明草案。票 17 才落盘，本票任何档都不自动写。
    IntakeBrief,
    RemotePublish,
    FinalAcceptance,
    BuiltinNever,
}

/// 自动通过时允许写到哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteScope {
    /// 不由本判定落盘（提案生效面是仓库白名单路径，回滚另走）。
    None,
    /// 当前项目：`.hexagon/skills`、项目 MCP 清单、项目 grants 表。
    Project,
}

/// 这一档、这一类确认怎么走。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessDisposition {
    Wait,
    AutoPass { scope: WriteScope },
}

/// `stored_rank` 是 `autonomy::rank`（0–4），不是 `execution_rank`。
/// 脏档（>4）与 L0–L3 一律等。
pub fn classify(stored_rank: u8, action: HarnessAction) -> HarnessDisposition {
    let allowed = matches!(
        action,
        HarnessAction::ProposalOwnerStamp
            | HarnessAction::SkillGrant
            | HarnessAction::McpGrant
            | HarnessAction::NlInstall
    );
    if stored_rank != 4 || !allowed {
        return HarnessDisposition::Wait;
    }
    let scope = match action {
        HarnessAction::SkillGrant | HarnessAction::McpGrant | HarnessAction::NlInstall => {
            WriteScope::Project
        }
        _ => WriteScope::None,
    };
    HarnessDisposition::AutoPass { scope }
}

pub fn auto_passes(stored_rank: u8, action: HarnessAction) -> bool {
    matches!(
        classify(stored_rank, action),
        HarnessDisposition::AutoPass { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn action() -> impl Strategy<Value = HarnessAction> {
        prop_oneof![
            Just(HarnessAction::ProposalOwnerStamp),
            Just(HarnessAction::SkillGrant),
            Just(HarnessAction::McpGrant),
            Just(HarnessAction::NlInstall),
            Just(HarnessAction::IntakeBrief),
            Just(HarnessAction::RemotePublish),
            Just(HarnessAction::FinalAcceptance),
            Just(HarnessAction::BuiltinNever),
        ]
    }

    fn is_allowlisted(action: HarnessAction) -> bool {
        matches!(
            action,
            HarnessAction::ProposalOwnerStamp
                | HarnessAction::SkillGrant
                | HarnessAction::McpGrant
                | HarnessAction::NlInstall
        )
    }

    proptest! {
        /// 不变量：自动通过当且仅当存储档恰好是 4 且动作在四件白名单。
        /// 开场草案、远程发布、最终验收、内置永不、L0–L3、脏档一律等。
        /// 授权和安装的自动通过写入面只能是当前项目。
        #[test]
        fn auto_pass_only_l4_allowlist_and_project_scope(
            rank in 0u8..8,
            action in action(),
        ) {
            let got = classify(rank, action);
            let expect_auto = rank == 4 && is_allowlisted(action);
            prop_assert_eq!(auto_passes(rank, action), expect_auto);
            match got {
                HarnessDisposition::Wait => prop_assert!(!expect_auto),
                HarnessDisposition::AutoPass { scope } => {
                    prop_assert!(expect_auto);
                    let project = matches!(
                        action,
                        HarnessAction::SkillGrant
                            | HarnessAction::McpGrant
                            | HarnessAction::NlInstall
                    );
                    prop_assert_eq!(scope == WriteScope::Project, project);
                    // 没有 Global 变体：授权/安装只能是 Project，提案盖章不带写入面。
                    if action == HarnessAction::ProposalOwnerStamp {
                        prop_assert_eq!(scope, WriteScope::None);
                    }
                }
            }
            if matches!(
                action,
                HarnessAction::IntakeBrief
                    | HarnessAction::RemotePublish
                    | HarnessAction::FinalAcceptance
                    | HarnessAction::BuiltinNever
            ) {
                prop_assert_eq!(got, HarnessDisposition::Wait);
            }
        }
    }
}
