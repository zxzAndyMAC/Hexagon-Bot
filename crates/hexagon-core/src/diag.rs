//! 诊断记录（2026-09-23 负责人裁决）。
//!
//! 分类只有四档。判定、槽位的正常回退、宿主记 Debug。拒绝，以及槽位上的失败，记 Warn。
//! 记录里是种类、id、分支、原因码、耗时。提示词、钥匙、工具输出和消息正文不进这里。
//! 测试不断言这些字符串。

#[allow(clippy::too_many_arguments)]
pub fn note(
    class: &str,
    warn: bool,
    project: Option<&str>,
    agent: Option<&str>,
    activation: Option<&str>,
    trace: Option<&str>,
    branch: &str,
    code: &str,
    started: std::time::Instant,
) {
    let ms = started.elapsed().as_millis();
    let line = format!(
        "diag class={class} project={} agent={} activation={} trace={} branch={branch} code={code} ms={ms}",
        project.unwrap_or("-"),
        agent.unwrap_or("-"),
        activation.unwrap_or("-"),
        trace.unwrap_or("-"),
    );
    if warn {
        log::warn!("{line}");
    } else {
        log::debug!("{line}");
    }
}
