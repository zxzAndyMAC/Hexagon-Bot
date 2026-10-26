//! 发布目标门面（owner-followups-1015 #14）：负责人登记 / 修改 / 删除发布目标，录入 / 替换 / 移除凭据。
//! 判定与持久化在 `crate::release_targets`；这里只把工作台的项目与凭据库接上。
//! 宿主壳层走 control 连接直接调同一组函数（不碰 `state.wb`），门面是 core 测试主接缝。

use super::*;
use crate::release_targets::{self as release, ReleaseTarget, ReleaseTargetDraft};

impl Workbench {
    /// 写面守门（owner-followups-1015 #15，ADR 0093 后续裁决「托管不得新登记、改命令或凭据」）：
    /// 裁决人不是负责人（托管 `managed:<grant>:<gen>` 等）时拒绝。今天没有托管路径会走到这里，
    /// 守门是为了以后加路径时默认拒绝。代价模型：误拒只让负责人以外的调用方报错；漏放是托管
    /// 自己改了它随后要代批执行的命令。
    fn owner_only(&self) -> Result<(), ApiError> {
        if self.decider() == "owner" {
            Ok(())
        } else {
            Err(release::ReleaseError::ReleaseOwnerOnly.into())
        }
    }

    pub fn release_targets(&self) -> Result<Vec<ReleaseTarget>, ApiError> {
        Ok(release::list(
            &self.db,
            &self.project_id,
            self.creds.as_ref(),
        )?)
    }

    pub fn save_release_target(
        &self,
        draft: &ReleaseTargetDraft,
    ) -> Result<ReleaseTarget, ApiError> {
        self.owner_only()?;
        Ok(release::save(
            &self.db,
            &self.project_id,
            self.creds.as_ref(),
            draft,
        )?)
    }

    pub fn remove_release_target(&self, id: &str) -> Result<(), ApiError> {
        self.owner_only()?;
        Ok(release::remove(
            &self.db,
            &self.project_id,
            self.creds.as_ref(),
            id,
        )?)
    }

    pub fn set_release_credential(
        &self,
        id: &str,
        credential: &str,
        secret: &str,
    ) -> Result<ReleaseTarget, ApiError> {
        self.owner_only()?;
        Ok(release::set_credential(
            &self.db,
            &self.project_id,
            self.creds.as_ref(),
            id,
            credential,
            secret,
        )?)
    }

    pub fn remove_release_credential(
        &self,
        id: &str,
        credential: &str,
    ) -> Result<ReleaseTarget, ApiError> {
        self.owner_only()?;
        Ok(release::remove_credential(
            &self.db,
            &self.project_id,
            self.creds.as_ref(),
            id,
            credential,
        )?)
    }
}
