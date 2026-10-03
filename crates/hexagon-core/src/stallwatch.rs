//! 失速监视（ADR 0074 / stall-watch 票 01–04）。
//!
//! 监视是工作台自己的机械判定，不是角色、不是 Agent。出处 ADR 0074：
//! 角色要被阶段、点名或项目经理派到才会醒——它自己不回复时，再加一个同类
//! 角色也醒不来；项目经理已经会「先不派活」把链停住，它自己也是一次会超时
//! 的模型调用。被否决：新设守护者角色；把监视扩进项目经理的职责。
//!
//! 只认三支：无回复、空转、调查超时。等网、用量打满、最终验收、复审返工、
//! 子代理未交还都不算。时钟只在没有进行中的回合之后起算，暂停与全员休眠期间
//! 不走、恢复后重新起算。三支都不拨阶段指针、不盖章、不远程发布。
//!
//! 代价模型（fail-closed 不对称性）：误报（false positive）= 一次多余的
//! 重触发或项目经理调查 + 一张打扰负责人的卡；漏报（false negative）= 项目
//! 静默卡住，要负责人自己发现，花一次人工。本判定偏向漏报：任何「现在该
//! 负责人动手」的迹象（待决卡、最终验收、中断的 run）都让监视静默——那时
//! 待决区已经有东西给负责人看，静默的代价最低。
//!
//! 本模块只放纯判定、段状态与读侧观测；动作（重触发、唤醒项目经理、入卡）
//! 在门面 `api.rs` 里执行，卡表写口仍归 `cards.rs`。

use crate::db::Db;
use std::time::{Duration, Instant};

/// 生产默认：60 秒（ADR 0074）。
pub const DEFAULT_BUDGET: Duration = Duration::from_secs(60);

/// 投给项目经理封闭选择的说话人标签（模型可见，英文，ADR 0071）。
pub const INVESTIGATION_SPEAKER: &str = "The workbench stall watch";

/// 时钟接缝：生产用单调时钟，测试注入可拨的假钟。
pub trait StallClock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl StallClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// 预算可注入：生产 60 秒，测试注入毫秒级或配假钟。
#[derive(Debug, Clone, Copy)]
pub struct StallPolicy {
    pub budget: Duration,
}

impl Default for StallPolicy {
    fn default() -> Self {
        Self {
            budget: DEFAULT_BUDGET,
        }
    }
}

/// 失速三支（词表定名，见 CONTEXT.md）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    NoReply,
    IdleSpin,
    InvestigationTimeout,
}

impl Branch {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoReply => "no_reply",
            Self::IdleSpin => "idle_spin",
            Self::InvestigationTimeout => "investigation_timeout",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "no_reply" => Some(Self::NoReply),
            "idle_spin" => Some(Self::IdleSpin),
            "investigation_timeout" => Some(Self::InvestigationTimeout),
            _ => None,
        }
    }
}

/// 监视看到的上一动静。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Last {
    /// 回合落地，但这位 Agent 没有留下可见回复（空文本、失败、被跳过都算）。
    Silent {
        agent_id: String,
        role: String,
        instruction: String,
    },
    /// 有可见回复。`progressed` = 这一回合里阶段指针/产物/待决卡变过。
    Replied { progressed: bool },
    /// 正常派活时项目经理「先不派活」。ADR 0074：第一次仍是空转信号。
    Held,
}

/// 判定只看动静的种类，不看正文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastKind {
    None,
    Silent,
    Replied { progressed: bool },
    Held,
}

impl Last {
    pub fn kind(&self) -> LastKind {
        match self {
            Self::Silent { .. } => LastKind::Silent,
            Self::Replied { progressed } => LastKind::Replied {
                progressed: *progressed,
            },
            Self::Held => LastKind::Held,
        }
    }
}

/// 一段失速的自动动作记账。进度一动、新的负责人消息或新的激活就清零。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Segment {
    /// 无回复已自动重触发过一次。
    pub retriggered: bool,
    /// 空转已自动唤醒过项目经理一次。再空转就入卡，不把空转再跑一遍。
    pub investigated: bool,
    /// 负责人在这段失速里已经点过一次「再试一次」。下一张卡只留「知道了」。
    pub retry_used: bool,
}

/// 监视的收束状态。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Open,
    /// 失速卡在队，等负责人。
    Carded,
    /// 失速收场：不入卡、不再叫醒，直到新的负责人消息或新的激活。
    Closed,
}

/// 上一动静 + 锚点时刻 + 当时的进度指纹。
#[derive(Debug, Clone)]
pub struct Seen {
    pub last: Last,
    /// 时钟起点：回合落地/先不派活的那一刻（暂停结束会重拨到结束那一刻）。
    pub at: Instant,
    pub snap: Fingerprint,
}

/// 工作台持有的监视状态（进程内）。
#[derive(Debug, Default)]
pub struct Watch {
    pub last: Option<Seen>,
    pub seg: Segment,
    pub status: Status,
    /// 看见过冻结：解冻那一拍把锚点重拨到当下（恢复后重新起算）。
    pub frozen_seen: bool,
    pub marker: Option<Marker>,
    /// 调查请求了但没开回合的时刻（票 04）。
    pub investigation_pending_since: Option<Instant>,
    /// 重触发回合的原指令：回合入口取走，记账写原指令而不是包了提示的那句，
    /// 否则再次重触发会一层层套提示。
    pub retrigger_original: Option<String>,
    /// 最近一句指令（回合入参或负责人的话），调查时带给项目经理。
    pub instruction: String,
}

impl Watch {
    /// 新一轮：清掉失速记账、收场状态和上一动静。
    /// 上一动静必须清——否则收场后负责人只唤醒、不跑回合时，旧的
    /// Silent 锚点还在，elapsed 早已超过预算，下一拍会立刻重触发
    /// （票 02：新的负责人消息或新的激活是重新计时，不是立刻出手）。
    pub fn reset_episode(&mut self) {
        self.seg = Segment::default();
        self.status = Status::Open;
        self.investigation_pending_since = None;
        self.last = None;
        self.retrigger_original = None;
    }
}

/// 读侧观测（纯数据，判定不碰库）。
#[derive(Debug, Clone, Copy)]
pub struct Obs {
    /// 有回合或封闭选择在飞。
    pub in_flight: bool,
    /// 顶栏暂停，或全员休眠。
    pub frozen: bool,
    /// 轮到负责人：有待决卡、停在盖章点（含最终验收）、或有中断的 run
    /// （进程被杀/等网超时挂起）。
    pub owner_waits: bool,
    /// 复审驳回后的返工还没交。
    pub review_rework: bool,
    /// 还有子代理没交还。
    pub subagent_pending: bool,
    /// 上一位没回复的 Agent 仍是激活状态。
    pub speaker_active: bool,
    /// 花名册上有项目经理。
    pub has_pm: bool,
    /// 距上一动静（或暂停结束）过去多久。
    pub elapsed: Duration,
    /// 调查已请求但没开回合：距请求过去多久。
    pub investigation_pending: Option<Duration>,
    pub budget: Duration,
}

/// 判定结果。`Wait` 的原因码进诊断，动作分支由门面执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Wait(&'static str),
    /// 无回复：对同一个 Agent 重触发一次。
    Retrigger,
    /// 空转：唤醒项目经理做封闭选择。
    Investigate,
    /// 交给负责人。`retry` = 卡上出不出「再试一次」。
    Card {
        branch: Branch,
        retry: bool,
    },
}

/// 失速判定（纯函数，D12 proptest 钉不变量）。顺序即优先级：
/// 在飞/冻结/收束/轮到负责人/子代理在外 → 等；然后才看时钟与三支。
pub fn judge(obs: &Obs, last: LastKind, seg: Segment, status: Status) -> Verdict {
    if obs.in_flight {
        return Verdict::Wait("in_flight");
    }
    if obs.frozen {
        return Verdict::Wait("frozen");
    }
    match status {
        Status::Closed => return Verdict::Wait("closed"),
        Status::Carded => return Verdict::Wait("carded"),
        Status::Open => {}
    }
    if obs.owner_waits {
        return Verdict::Wait("owner_waits");
    }
    if obs.subagent_pending {
        return Verdict::Wait("subagent_pending");
    }
    // 票 04：调查根本没开回合 → 等满预算入卡，不自动重试调查。
    if let Some(waited) = obs.investigation_pending {
        if waited < obs.budget {
            return Verdict::Wait("investigation_pending");
        }
        return Verdict::Card {
            branch: Branch::InvestigationTimeout,
            retry: !seg.retry_used,
        };
    }
    if last == LastKind::None {
        return Verdict::Wait("no_activity");
    }
    if obs.elapsed < obs.budget {
        return Verdict::Wait("clock");
    }
    match last {
        LastKind::None => Verdict::Wait("no_activity"),
        LastKind::Silent => {
            if !obs.speaker_active {
                return Verdict::Wait("speaker_asleep");
            }
            if !seg.retriggered {
                Verdict::Retrigger
            } else {
                // 票 02：重触发后仍无回复 → 入卡，不唤醒项目经理。
                Verdict::Card {
                    branch: Branch::NoReply,
                    retry: !seg.retry_used,
                }
            }
        }
        LastKind::Replied { progressed: true } => Verdict::Wait("progressed"),
        LastKind::Replied { progressed: false } | LastKind::Held => {
            if obs.review_rework {
                return Verdict::Wait("review_rework");
            }
            // 票 03：花名册没有项目经理 → 入卡，卡上没有「再试一次」（没有谁可以再唤醒）。
            if !obs.has_pm {
                return Verdict::Card {
                    branch: Branch::IdleSpin,
                    retry: false,
                };
            }
            if !seg.investigated {
                Verdict::Investigate
            } else {
                Verdict::Card {
                    branch: Branch::IdleSpin,
                    retry: !seg.retry_used,
                }
            }
        }
    }
}

/// 进度指纹：阶段指针、产物、待决卡。三者都没变 = 没有进度。
/// 没有阶段指针（快速通道）时该段为空串，按「指针没变」处理。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fingerprint {
    stage: String,
    artifacts: String,
    cards: Vec<String>,
}

pub fn fingerprint(db: &Db, project_id: &str) -> Result<Fingerprint, crate::cards::CardsError> {
    let stage: String = db.conn().query_row(
        "SELECT COALESCE((SELECT id||':'||state FROM stage_runs
                          WHERE project_id=?1 ORDER BY rowid DESC LIMIT 1), '')",
        [project_id],
        |r| r.get(0),
    )?;
    let artifacts: String = db.conn().query_row(
        "SELECT COALESCE(group_concat(k, ','), '') FROM
           (SELECT id||':'||version||':'||status AS k FROM artifacts
            WHERE project_id=?1 ORDER BY id)",
        [project_id],
        |r| r.get(0),
    )?;
    Ok(Fingerprint {
        stage,
        artifacts,
        cards: crate::cards::all_queued_ids(db, project_id)?,
    })
}

/// 新一轮的边界：负责人新消息、新开的阶段 run、负责人亲手唤醒。
/// 变了 = 上一轮（含失速收场）结束，重新计时。项目经理派活时的
/// 「点名唤醒」不算——否则调查派活就能把自己的记账清零，失速无上限地循环。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    owner_msg: i64,
    runs: i64,
    owner_wake: i64,
}

pub fn marker(db: &Db, project_id: &str) -> Result<Marker, rusqlite::Error> {
    let q = |sql: &str| -> Result<i64, rusqlite::Error> {
        db.conn().query_row(sql, [project_id], |r| r.get(0))
    };
    Ok(Marker {
        owner_msg: q(
            "SELECT COALESCE(MAX(id),0) FROM messages WHERE project_id=?1 AND author='owner'",
        )?,
        runs: q("SELECT COUNT(*) FROM stage_runs WHERE project_id=?1")?,
        owner_wake: q("SELECT COALESCE(MAX(id),0) FROM events
                       WHERE project_id=?1 AND kind='agent_activated'
                         AND json_extract(payload,'$.by')='owner'")?,
    })
}

/// 顶栏暂停或全员休眠。全员休眠看事件而不是 agents.status 计数：
/// 最后一次 team_slept（负责人全员休眠、包跑完、用量打满）之后没有人再被激活。
/// 被否决：数 status='active' 为零——新建的快速通道项目谁都还没派到，
/// 全员默认 sleeping，那不是有意的休眠，照样要计时（修前实测：快速通道
/// 项目经理先不派活后永远判冻结，空转调查不来）。
pub fn frozen(db: &Db, project_id: &str) -> Result<bool, crate::orchestra::OrchError> {
    if crate::orchestra::is_paused(db, project_id)? {
        return Ok(true);
    }
    let latest = |kind: &str| -> Result<i64, rusqlite::Error> {
        db.conn().query_row(
            "SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND kind=?2",
            rusqlite::params![project_id, kind],
            |r| r.get(0),
        )
    };
    let slept = latest("team_slept")?;
    Ok(slept > 0 && slept > latest("agent_activated")?)
}

/// 轮到负责人：待决卡、盖章点（含最终验收）、中断的 run。
pub fn owner_waits(db: &Db, project_id: &str) -> Result<bool, crate::cards::CardsError> {
    if crate::cards::count_queued(db, project_id, None)? > 0 {
        return Ok(true);
    }
    let n: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM stage_runs
         WHERE project_id=?1 AND state IN ('waiting_stamp','interrupted')",
        [project_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Live acceptance 2026-10-01: completed-pack screenshot follow-ups reached
/// PM HOLD, then replayed after 60s. Use the orchestra's actual PackFinished
/// receipt, not an absent active run (which also describes an unstarted pack).
/// False negatives cost one rescue; false positives suppress unfinished work,
/// so missing receipts and any subsequent stage opening fail closed.
pub(crate) fn pack_finished(db: &Db, project: &str) -> Result<bool, rusqlite::Error> {
    db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM events e WHERE e.project_id=?1
         AND e.kind='team_slept' AND json_extract(e.payload,'$.reason')='pack finished'
         AND NOT EXISTS(SELECT 1 FROM events later WHERE later.project_id=e.project_id
                        AND later.kind='stage_started' AND later.id>e.id))",
        [project],
        |row| row.get(0),
    )
}

/// Only a successful visible reply followed by a normal HOLD can settle a
/// completed pack's supplemental episode. Silent/error turns retain rescue.
pub(crate) fn supplemental_settled(
    finished_pack: bool,
    replied: bool,
    held: bool,
    outstanding: bool,
) -> bool {
    finished_pack && replied && held && !outstanding
}

/// 复审返工：当前 run 里某份产物最近一次复审结论是驳回。
pub fn review_rework(db: &Db, project_id: &str) -> Result<bool, rusqlite::Error> {
    let n: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM events e
         WHERE e.project_id=?1 AND e.kind='review_rejected'
           AND e.stage_run_id = (SELECT id FROM stage_runs
                                 WHERE project_id=?1 AND state IN ('active','waiting_stamp')
                                 ORDER BY seq DESC LIMIT 1)
           AND e.id = (SELECT MAX(id) FROM events
                       WHERE project_id=e.project_id AND stage_run_id=e.stage_run_id
                         AND kind IN ('review_passed','review_rejected','review_skipped')
                         AND json_extract(payload,'$.artifact_kind')
                             = json_extract(e.payload,'$.artifact_kind'))",
        [project_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 重触发的简报（模型可见，英文，ADR 0071）。原指令逐字带上：
/// 上一回合已经跑完，简报游标推进过，不能像恢复重触发那样靠游标重读。
pub fn retrigger_input(instruction: &str) -> String {
    format!(
        "The workbench noticed your last turn ended without a visible reply. \
         Pick up the instruction below now: check what is already on disk first, \
         then answer on the timeline.\n\nOriginal instruction:\n{instruction}"
    )
}

/// 调查的状态句（进项目经理的封闭选择；被派到的角色也拿它当指令）。
pub fn investigation_body(budget: Duration, instruction: &str) -> String {
    format!(
        "Nothing has moved for {}s after the last reply: the stage pointer, the artifacts \
         and the pending cards are all unchanged. Decide who should act next.\n\n\
         Last instruction:\n{instruction}",
        budget.as_secs().max(1)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn obs() -> Obs {
        Obs {
            in_flight: false,
            frozen: false,
            owner_waits: false,
            review_rework: false,
            subagent_pending: false,
            speaker_active: true,
            has_pm: true,
            elapsed: Duration::from_secs(61),
            investigation_pending: None,
            budget: DEFAULT_BUDGET,
        }
    }

    #[test]
    fn silent_retriggers_once_then_cards() {
        let s = Segment::default();
        assert_eq!(
            judge(&obs(), LastKind::Silent, s, Status::Open),
            Verdict::Retrigger
        );
        let s = Segment {
            retriggered: true,
            ..s
        };
        assert_eq!(
            judge(&obs(), LastKind::Silent, s, Status::Open),
            Verdict::Card {
                branch: Branch::NoReply,
                retry: true
            }
        );
        let s = Segment {
            retry_used: true,
            ..s
        };
        assert_eq!(
            judge(&obs(), LastKind::Silent, s, Status::Open),
            Verdict::Card {
                branch: Branch::NoReply,
                retry: false
            }
        );
    }

    #[test]
    fn idle_investigates_once_and_no_pm_cards_without_retry() {
        for last in [LastKind::Held, LastKind::Replied { progressed: false }] {
            assert_eq!(
                judge(&obs(), last, Segment::default(), Status::Open),
                Verdict::Investigate
            );
            let o = Obs {
                has_pm: false,
                ..obs()
            };
            assert_eq!(
                judge(&o, last, Segment::default(), Status::Open),
                Verdict::Card {
                    branch: Branch::IdleSpin,
                    retry: false
                }
            );
        }
        assert_eq!(
            judge(
                &obs(),
                LastKind::Replied { progressed: true },
                Segment::default(),
                Status::Open
            ),
            Verdict::Wait("progressed")
        );
    }

    #[test]
    fn unopened_investigation_cards_after_budget() {
        let o = Obs {
            investigation_pending: Some(Duration::from_secs(10)),
            ..obs()
        };
        assert_eq!(
            judge(&o, LastKind::Held, Segment::default(), Status::Open),
            Verdict::Wait("investigation_pending")
        );
        let o = Obs {
            investigation_pending: Some(Duration::from_secs(60)),
            ..obs()
        };
        assert_eq!(
            judge(&o, LastKind::Held, Segment::default(), Status::Open),
            Verdict::Card {
                branch: Branch::InvestigationTimeout,
                retry: true
            }
        );
    }

    fn arb_last() -> impl Strategy<Value = LastKind> {
        prop_oneof![
            Just(LastKind::None),
            Just(LastKind::Silent),
            Just(LastKind::Held),
            any::<bool>().prop_map(|p| LastKind::Replied { progressed: p }),
        ]
    }

    fn arb_status() -> impl Strategy<Value = Status> {
        prop_oneof![
            Just(Status::Open),
            Just(Status::Carded),
            Just(Status::Closed)
        ]
    }

    prop_compose! {
        fn arb_obs()(
            in_flight in any::<bool>(),
            frozen in any::<bool>(),
            owner_waits in any::<bool>(),
            review_rework in any::<bool>(),
            subagent_pending in any::<bool>(),
            speaker_active in any::<bool>(),
            has_pm in any::<bool>(),
            elapsed_ms in 0u64..200_000,
            pending in proptest::option::of(0u64..200_000),
            budget_ms in 1u64..120_000,
        ) -> Obs {
            Obs {
                in_flight, frozen, owner_waits, review_rework, subagent_pending,
                speaker_active, has_pm,
                elapsed: Duration::from_millis(elapsed_ms),
                investigation_pending: pending.map(Duration::from_millis),
                budget: Duration::from_millis(budget_ms),
            }
        }
    }

    prop_compose! {
        fn arb_seg()(r in any::<bool>(), i in any::<bool>(), u in any::<bool>()) -> Segment {
            Segment { retriggered: r, investigated: i, retry_used: u }
        }
    }

    fn acts(v: Verdict) -> bool {
        !matches!(v, Verdict::Wait(_))
    }

    proptest! {
        /// 回合在飞不计时（票 01 不变量）：不管其它观测，在飞只能等。
        #[test]
        fn turn_in_flight_never_acts(o in arb_obs(), l in arb_last(), s in arb_seg(), st in arb_status()) {
            let o = Obs { in_flight: true, ..o };
            prop_assert_eq!(judge(&o, l, s, st), Verdict::Wait("in_flight"));
        }

        /// 暂停不计时（票 01 不变量）：暂停/全员休眠期间不重触发、不调查、不入卡。
        #[test]
        fn frozen_never_acts(o in arb_obs(), l in arb_last(), s in arb_seg(), st in arb_status()) {
            let o = Obs { in_flight: false, frozen: true, ..o };
            prop_assert_eq!(judge(&o, l, s, st), Verdict::Wait("frozen"));
        }

        /// 等负责人的卡不算空转（票 03 不变量）：轮到负责人时监视不出手。
        #[test]
        fn owner_waiting_never_acts(o in arb_obs(), l in arb_last(), s in arb_seg(), st in arb_status()) {
            let o = Obs { owner_waits: true, ..o };
            prop_assert!(!acts(judge(&o, l, s, st)));
        }

        /// 复审返工不算空转（票 03 不变量）：返工期间不唤醒项目经理，也不按空转入卡。
        #[test]
        fn review_rework_never_idle_spins(o in arb_obs(), l in arb_last(), s in arb_seg(), st in arb_status()) {
            let o = Obs { review_rework: true, investigation_pending: None, ..o };
            let v = judge(&o, l, s, st);
            prop_assert!(v != Verdict::Investigate);
            let idle_card = matches!(v, Verdict::Card { branch: Branch::IdleSpin, .. });
            prop_assert!(!idle_card, "idle-spin card during rework: {:?}", v);
        }

        /// 时钟没到不出手（调查未开回合除外，它有自己的计时）。
        #[test]
        fn before_budget_waits(o in arb_obs(), l in arb_last(), s in arb_seg(), st in arb_status(), frac in 0u32..1000) {
            let elapsed = o.budget * frac / 1000;
            let o = Obs { elapsed, investigation_pending: None, ..o };
            prop_assert!(!acts(judge(&o, l, s, st)));
        }

        /// 收场与卡在队时不再出手——不叫醒、不第二张卡。
        #[test]
        fn closed_or_carded_never_acts(o in arb_obs(), l in arb_last(), s in arb_seg()) {
            prop_assert!(!acts(judge(&o, l, s, Status::Closed)));
            prop_assert!(!acts(judge(&o, l, s, Status::Carded)));
        }

        /// 再试一次同一段失速最多一轮；无项目经理时卡上没有再试一次。
        #[test]
        fn retry_bounded(o in arb_obs(), l in arb_last(), s in arb_seg(), st in arb_status()) {
            if let Verdict::Card { branch, retry } = judge(&o, l, s, st) {
                if s.retry_used { prop_assert!(!retry); }
                if branch == Branch::IdleSpin && !o.has_pm { prop_assert!(!retry); }
            }
        }

        /// 无回复不唤醒项目经理；空转不重跑同一人。
        #[test]
        fn branches_do_not_cross(o in arb_obs(), s in arb_seg(), st in arb_status()) {
            let o = Obs { investigation_pending: None, ..o };
            prop_assert!(judge(&o, LastKind::Silent, s, st) != Verdict::Investigate);
            for l in [LastKind::Held, LastKind::Replied { progressed: false }, LastKind::Replied { progressed: true }] {
                prop_assert!(judge(&o, l, s, st) != Verdict::Retrigger);
            }
        }

        /// 自动动作每段各至多一次：记账已满时不会再自动重触发/调查。
        #[test]
        fn auto_actions_once_per_segment(o in arb_obs(), l in arb_last(), st in arb_status(), u in any::<bool>()) {
            let s = Segment { retriggered: true, investigated: true, retry_used: u };
            let v = judge(&o, l, s, st);
            prop_assert!(v != Verdict::Retrigger && v != Verdict::Investigate);
        }
    }
}

#[cfg(test)]
mod supplemental_properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn supplemental_close_requires_every_host_fact(
            pack in any::<bool>(), reply in any::<bool>(), held in any::<bool>(), pending in any::<bool>()
        ) {
            let closes = supplemental_settled(pack, reply, held, pending);
            prop_assert_eq!(closes, pack && reply && held && !pending);
            if !pack || !reply || !held || pending { prop_assert!(!closes); }
        }
    }
}
