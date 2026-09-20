//! Hexagon-Bot 核。
//!
//! 全部业务逻辑所在：编排内核、回合内核、产物管道、权限管线、用量账本、
//! 轨迹、MCP 宿主、git 管道、凭据、远程发布、改进提案、自治档位。
//! Tauri/WebView 只是薄壳，经进程内 API 边界（本 crate 的公开面）与核交互。
//! 该边界同时是全项目唯一的测试主接缝（见 spec「Testing Decisions」）。

pub mod api;
pub mod artifacts;
pub mod autonomy;
pub mod cards;
pub mod commands;
pub mod credentials;
pub mod db;
pub mod errcode;
pub mod git;
pub mod install;
pub mod invariant;
pub mod judge;
pub mod mcp;
pub mod orchestra;
pub mod packedit;
pub mod permissions;
pub mod policydev;
pub mod presets;
pub mod proposals;
pub mod provenance;
pub mod provider;
pub mod provider_admin;
pub mod provider_config;
pub mod publish;
pub mod replay;
pub mod research;
pub mod review;
pub mod reviewer;
pub mod roles;
pub mod scenario;
pub mod setup;
pub mod skills;
pub mod tools;
pub mod trace;
pub mod turn;
pub mod usage;

/// 单库单项目约定：每个项目目录一个 state.db，项目 id 恒为 "p1"。
/// 壳层旁路写入（send_message_side/pause/resume）与 core 共用此常量——
/// 两侧曾各自硬编码 "p1" 共 8 处（arch-review 票 01 / 诊断卡 D17）。
pub const PROJECT_ID: &str = "p1";

/// IPC 链路自检：核 → 壳 → WebView 的最小证明。
pub fn ping() -> String {
    format!("hexagon-core v{} ok", env!("CARGO_PKG_VERSION"))
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_proves_ipc_path() {
        assert!(ping().contains("ok"));
        assert!(ping().contains(version()));
    }
}
