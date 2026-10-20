//! 发布目标门面（owner-followups-1015 #14）：负责人登记 / 修改 / 删除发布目标，录入 / 替换 / 移除凭据。
//! 判定与持久化在 `crate::release_targets`；这里只把工作台的项目与凭据库接上。
//! 宿主壳层走 control 连接直接调同一组函数（不碰 `state.wb`），门面是 core 测试主接缝。

use super::*;
use crate::release_targets::{self as release, ReleaseTarget, ReleaseTargetDraft};

impl Workbench {
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
        Ok(release::save(
            &self.db,
            &self.project_id,
            self.creds.as_ref(),
            draft,
        )?)
    }

    pub fn remove_release_target(&self, id: &str) -> Result<(), ApiError> {
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
        Ok(release::remove_credential(
            &self.db,
            &self.project_id,
            self.creds.as_ref(),
            id,
            credential,
        )?)
    }
}
