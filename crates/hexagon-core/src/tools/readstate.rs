//! 先读后改判定（prompt-engineering 票 02）。
//!
//! `fs_patch` 与覆盖已存在文件的 `fs_write` 要求本次激活内用 `fs_read` 读过
//! 目标，且读后文件没变。出处：Claude Code 的 FileEditTool 同款契约
//! （readFileState + 「File has been modified since read」）；hexagon 多角色
//! 并行改同一仓库，基于旧内容覆盖别人改动的风险比单代理 CLI 更高。
//!
//! 代价模型：漏判 = 一次基于旧内容的覆盖，可能吞掉别的角色或负责人的改动，
//! 事后很难察觉；误判 = 模型多读一次文件。偏向误判——mtime 与长度任一不同
//! 就算变过；读取时指纹取在读之前。残余漏判：mtime 粒度粗的文件系统上，
//! 同一时间刻内的等长改动认不出（不做内容哈希：每次编辑都要重读全文）。
//!
//! 激活已读检查在初次派发前做（`Tool::precondition`）。可靠性票15修复
//! 旧批准路径跳过校验导致覆盖等待期间编辑的问题：writeguard将完整目标
//! 摘要持久绑定到动作，批准后用该证据复验，不依赖控制连接的新空账本。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 文件指纹：修改时间（纳秒）+ 字节长度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub mtime_ns: u128,
    pub len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    NotRead,
    Changed,
}

impl Verdict {
    pub fn code(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NotRead => "not_read",
            Self::Changed => "changed_since_read",
        }
    }
}

/// 纯判定：`recorded` 是账本里的指纹，`current` 是目标此刻的指纹
/// （None = 目标不存在）。
pub fn judge(recorded: Option<Stamp>, current: Option<Stamp>) -> Verdict {
    match (recorded, current) {
        (_, None) => Verdict::Ok,
        (None, Some(_)) => Verdict::NotRead,
        (Some(r), Some(c)) if r == c => Verdict::Ok,
        (Some(_), Some(_)) => Verdict::Changed,
    }
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    let mtime_ns = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(Stamp {
        mtime_ns,
        len: m.len(),
    })
}

/// 激活级已读账本：ctx 每回合新建，账本随之清空。
#[derive(Clone, Default)]
pub struct ReadLedger(Arc<Mutex<HashMap<PathBuf, Stamp>>>);

impl ReadLedger {
    /// 自己刚写过：记下此刻的指纹。
    pub fn record(&self, path: &Path) {
        if let Some(s) = stamp(path) {
            self.record_stamp(path, s);
        }
    }

    /// 读过：记下读之前取的指纹。
    pub fn record_stamp(&self, path: &Path, s: Stamp) {
        self.0.lock().unwrap().insert(path.to_path_buf(), s);
    }

    pub fn check(&self, path: &Path) -> Verdict {
        let recorded = self.0.lock().unwrap().get(path).copied();
        judge(recorded, stamp(path))
    }
}
