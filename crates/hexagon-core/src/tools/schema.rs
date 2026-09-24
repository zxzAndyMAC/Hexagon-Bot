//! 工具入参的 JSON Schema 校验（prompt-engineering 票 03）。
//!
//! 位置：`call_with_seq` 里早于内置 deny 与权限判定。旧写法入参只在执行时
//! 由 `str_arg` 检查，排在权限之后——缺参数的 `bash` 也先弹卡，负责人批了
//! 才报错（2026-09-24 对照 Claude Code 研究发现；Claude Code 同样是
//! 结构校验 → 语义校验 → 权限）。
//!
//! 代价模型：漏判（坏参数放过去）= 执行时报一次错，模型下一轮自修；
//! 误判（合法参数被拦）= 模型卡在一个它改不了的错误上并触发同错熔断。
//! 偏向漏判：用规范一致的 `jsonschema` 而不是自研半套（自研在 `$ref`、
//! `anyOf`、类型联合上最易误判——MCP 的 schema 来自第三方，写法不受控）；
//! schema 自身编译不过的工具跳过校验，而不是拒掉全部调用。
//!
//! 依赖关闭默认特性：默认 `resolve-http` 会按第三方 schema 里的远程
//! `$ref` 让宿主发 HTTP 请求。关着时远程引用编译失败 → 走跳过分支。

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Compiled = Option<Arc<jsonschema::Validator>>;

/// 回喂模型的错误条数上限：多了只是噪音，前几条足够改参数。
const MAX_REPORTED: usize = 5;

/// 按工具名缓存编译结果（`None` = schema 编译失败，跳过校验）。
/// 惰性编译：子代理注册表按名直拷 mcp 工具不经 `register`，惰性才不漏。
#[derive(Default)]
pub(crate) struct SchemaCache {
    compiled: Mutex<HashMap<String, Compiled>>,
}

impl SchemaCache {
    /// 同名工具重注册（MCP 重连换了 schema）时丢掉旧编译结果。
    pub(crate) fn invalidate(&self, name: &str) {
        self.compiled.lock().unwrap().remove(name);
    }

    /// Err(文案) = 入参不合 schema；Ok = 合法或该工具跳过校验。
    pub(crate) fn validate(&self, name: &str, schema: &Value, input: &Value) -> Result<(), String> {
        let compiled = {
            let mut map = self.compiled.lock().unwrap();
            map.entry(name.to_string())
                .or_insert_with(|| compile(name, schema))
                .clone()
        };
        match compiled {
            Some(v) => describe(&v, input),
            None => Ok(()),
        }
    }
}

fn compile(name: &str, schema: &Value) -> Compiled {
    let started = std::time::Instant::now();
    match jsonschema::validator_for(schema) {
        Ok(v) => Some(Arc::new(v)),
        Err(_) => {
            crate::diag::note(
                crate::diag::CLASS_HOST,
                false,
                None,
                None,
                None,
                None,
                "tool_schema",
                &format!("schema_uncompilable:{name}"),
                started,
            );
            None
        }
    }
}

fn describe(v: &jsonschema::Validator, input: &Value) -> Result<(), String> {
    let errs: Vec<String> = v
        .iter_errors(input)
        .take(MAX_REPORTED)
        .map(|e| {
            let at = e.instance_path().as_str();
            let at = if at.is_empty() { "(root)" } else { at };
            format!("{at}: {e}")
        })
        .collect();
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("; "))
    }
}

/// 一次性校验（不缓存），供属性测试钉不变量。schema 编译失败按跳过处理。
#[cfg(test)]
pub(crate) fn check(schema: &Value, input: &Value) -> Result<(), String> {
    match jsonschema::validator_for(schema) {
        Ok(v) => describe(&v, input),
        Err(_) => Ok(()),
    }
}
