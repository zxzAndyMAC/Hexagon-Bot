//! 盖章门判定（hands-free 票 02 / ADR 0059）。
//!
//! 只回答「这一道盖章点自动通过，还是等负责人」。不读库，也不放行安全网、
//! 权限、提案或安装——那些仍看 `autonomy::execution_rank`（封顶 L2，票 03/04）。
//! 盖章自动通过读的是存储档，不是执行档。被否决：把执行档封顶抬到 3。
//! 那样安全网和新权限会在本票里一起放行。
//!
//! 代价：非最终盖章点误判成等人，负责人多回来一次（false negative，可补盖）。
//! 最终验收或 L0–L2 误判成自动通过，是一次未审副作用（false positive）。
//! 实现偏向等人：档位不是恰好 3 或 4、下标不是盖章点、或这是最后一个盖章点，一律等。

/// 一道盖章点在当前存储档下怎么走。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampDisposition {
    /// 排队等负责人。最终验收，以及 L0–L2 的每一个盖章点。
    Wait,
    /// L3/L4 的非最终盖章点。轨迹必须和人工盖章区分开。
    AutoPass,
}

/// `seq` 是不是流程包里最后一个盖章点。没有盖章点时为 false。
pub fn is_final_stamp(stamp_flags: &[bool], seq: usize) -> bool {
    match stamp_flags.iter().rposition(|s| *s) {
        Some(last) => last == seq,
        None => false,
    }
}

/// `stored_rank` 是 `autonomy::rank`（0–4），不是 `execution_rank`。
/// `stamp_flags[i]` 表示该阶段是盖章点。`seq` 越界或不是盖章点 → 等。
///
/// 快速通道没有更早的门：合入基线用单元素 `[true]` 调用，恒为最终验收。
pub fn classify_stamp(stored_rank: u8, stamp_flags: &[bool], seq: usize) -> StampDisposition {
    let is_stamp = stamp_flags.get(seq).copied().unwrap_or(false);
    if !is_stamp || is_final_stamp(stamp_flags, seq) {
        return StampDisposition::Wait;
    }
    // 脏档（>4 或词表外）与 L0–L2 不自动盖章。只有存储档恰好 L3/L4。
    if stored_rank == 3 || stored_rank == 4 {
        StampDisposition::AutoPass
    } else {
        StampDisposition::Wait
    }
}

/// 快速通道的合入基线就是最终验收。任何存储档都不自动合入。
pub fn may_auto_fastpath_merge(stored_rank: u8) -> bool {
    classify_stamp(stored_rank, &[true], 0) == StampDisposition::AutoPass
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_flags() -> impl Strategy<Value = Vec<bool>> {
        proptest::collection::vec(any::<bool>(), 0..12)
    }

    proptest! {
        /// 不变量：自动通过当且仅当存储档恰好 L3/L4、该下标是盖章点、且不是最后一道。
        /// 最终验收、L0–L2、脏档、非盖章点、越界下标一律等。
        #[test]
        fn auto_pass_only_non_final_at_l3_l4(
            rank in 0u8..8,
            flags in arb_flags(),
            seq in 0usize..16,
        ) {
            let got = classify_stamp(rank, &flags, seq);
            let is_stamp = flags.get(seq).copied().unwrap_or(false);
            let high = rank == 3 || rank == 4;
            let expect_auto = high && is_stamp && !is_final_stamp(&flags, seq);
            prop_assert_eq!(got == StampDisposition::AutoPass, expect_auto);
            if is_final_stamp(&flags, seq) || !high {
                prop_assert_eq!(got, StampDisposition::Wait);
            }
        }

        /// 快速通道合入 = 唯一一道门。任何档位都不自动合入。
        #[test]
        fn fastpath_baseline_never_auto_merges(rank in 0u8..8) {
            prop_assert!(!may_auto_fastpath_merge(rank));
            prop_assert_eq!(classify_stamp(rank, &[true], 0), StampDisposition::Wait);
        }
    }
}
