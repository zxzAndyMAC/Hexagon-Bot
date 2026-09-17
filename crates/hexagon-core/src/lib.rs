//! Hexagon-Bot 核。
//!
//! 全部业务逻辑所在：编排内核、回合内核、产物管道、权限管线、用量账本、
//! 轨迹、MCP 宿主、git 管道、凭据、远程发布、改进提案、自治档位。
//! Tauri/WebView 只是薄壳，经进程内 API 边界（本 crate 的公开面）与核交互。
//! 该边界同时是全项目唯一的测试主接缝（见 spec「Testing Decisions」）。

pub mod api;
pub mod artifacts;
pub mod db;
pub mod mcp;
pub mod orchestra;
pub mod permissions;
pub mod provider;
pub mod review;
pub mod scenario;
pub mod tools;
pub mod trace;
pub mod turn;
pub mod usage;

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
