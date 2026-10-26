//! 发布门面（owner-followups-1015 #15）：负责人对已登记的发布目标发起发布 → 发布确认卡 → 确认后执行。
//! 判定、执行与轨迹在 `crate::release`；这里只把工作台的项目、凭据库与裁决人接上。
//! 宿主壳层的 confirm 走自开账本连接（长命令不占工作台锁），门面是 core 测试主接缝。

use super::*;
use crate::release::{self as release, ReleaseOutcome};

impl Workbench {
    /// 发起发布：只入队发布确认卡（同目标已有待决卡则返回那张）。
    pub fn request_release(&self, target_id: &str) -> Result<String, ApiError> {
        Ok(release::request(
            &self.db,
            &self.project_id,
            target_id,
            &self.decider(),
        )?)
    }

    /// 确认发布：前置闸（目标未改、凭据齐）→ 认领 → 执行登记的命令。
    pub fn confirm_release(&self, qid: &str) -> Result<ReleaseOutcome, ApiError> {
        Ok(release::confirm_by(
            &self.db,
            &self.project_id,
            qid,
            self.creds.as_ref(),
            &self.decider(),
            release::RELEASE_TIMEOUT,
        )?)
    }

    /// 批准「登记发布目标」卡（票 16）：只有负责人（`release_chat::approve` 守门）；批准才落库。
    pub fn approve_release_target_card(
        &self,
        qid: &str,
    ) -> Result<crate::release_targets::ReleaseTarget, ApiError> {
        Ok(crate::release_chat::approve(
            &self.db,
            &self.project_id,
            qid,
            self.creds.as_ref(),
            &self.decider(),
        )?)
    }

    /// 驳回「登记发布目标」卡：什么都不登记。
    pub fn reject_release_target_card(&self, qid: &str) -> Result<(), ApiError> {
        Ok(crate::release_chat::reject(
            &self.db,
            &self.project_id,
            qid,
            &self.decider(),
        )?)
    }

    pub fn reject_release(&self, qid: &str) -> Result<(), ApiError> {
        Ok(release::reject_by(
            &self.db,
            &self.project_id,
            qid,
            &self.decider(),
        )?)
    }
}
