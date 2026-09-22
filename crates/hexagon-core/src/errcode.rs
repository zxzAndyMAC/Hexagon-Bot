//! IPC 错误码（ADR 0054，arch-review 票 06）：每条命令错误出列即
//! `{code, message}`——本模块只负责 `code` 半边，`message` 透传
//! `Display` 原文（含参数细节），UI 对已知 code 走 `errors.<code>` i18n、
//! 未知 code 渲染 message。
//!
//! 码值规则：默认取**变体稳定标识**（`Debug` 头 → snake_case），
//! `#[from]`/透明包装层显式递归到内层码——「中途经过哪个包装枚举」不是
//! 用户要面对的语义，叶子才是。被否决的替代：字符串硬编码表（变体重命名
//! 时静默漂移）、kind 枚举（24 个枚举 × 百余变体，表会跟实现脱钩）。

use std::fmt::Debug;

/// 变体稳定标识 → snake_case。Debug 头取 `Variant(`/`Variant {`/`Variant`
/// 的首词；连续大写按单词边界切（`NoActiveStage` → `no_active_stage`）。
/// 枚举改名属编译面变更，码表随之走——这正是「稳定标识」的语义。
pub fn variant_code<E: Debug + ?Sized>(e: &E) -> String {
    let dbg = format!("{e:?}");
    let head = dbg
        .split(|c: char| c == '(' || c == '{' || c.is_whitespace())
        .next()
        .unwrap_or(dbg.as_str());
    let mut out = String::with_capacity(head.len() + 4);
    for (i, ch) in head.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// IPC 错误码。叶变体走默认实现（变体名 → snake_case）；含 `#[from]`/
/// 手工包装的枚举用 `impl_error_code!` 声明递归臂。
pub trait ErrorCode: Debug {
    fn code(&self) -> String {
        variant_code(self)
    }
}

/// 为错误枚举生成 `ErrorCode` impl：列出的变体递归到内层 `code()`，
/// 其余（叶变体）回落 `variant_code`。列错名的代价是编译错，
/// 漏列 from 变体的代价是码停在包装层（如 `tool` 而非 `path_escape`）——
/// 加新 `#[from]` 变体时记得登记。
macro_rules! impl_error_code {
    ($t:ty) => {
        impl $crate::errcode::ErrorCode for $t {}
    };
    ($t:ty, $($v:ident),+ $(,)?) => {
        impl $crate::errcode::ErrorCode for $t {
            // 全变体皆递归的枚举（如 DbError）`_` 臂不可达——属正常形态。
            #[allow(unreachable_patterns)]
            fn code(&self) -> String {
                match self {
                    $(Self::$v(e) => e.code(),)+
                    _ => $crate::errcode::variant_code(self),
                }
            }
        }
    };
}

// 外来错误类型：码面统一为底层类别词（参数已在 message 里）。
impl ErrorCode for rusqlite::Error {
    fn code(&self) -> String {
        "sqlite".into()
    }
}
impl ErrorCode for std::io::Error {
    fn code(&self) -> String {
        "io".into()
    }
}
impl ErrorCode for serde_json::Error {
    fn code(&self) -> String {
        "json".into()
    }
}

impl_error_code!(
    crate::api::ApiError,
    Db,
    Trace,
    Orch,
    Turn,
    Tool,
    Artifact,
    Publish,
    Autonomy,
    Proposal,
    Review,
    PolicyDev,
    Sqlite,
    Cards,
    Json,
    Io,
    Install,
    Roles,
    PackEdit,
    Judge,
);
impl_error_code!(crate::judge::JudgeError, Sqlite, Io, Trace, Cards,);
impl_error_code!(crate::artifacts::ArtifactError, Tool, Trace, Db, Sqlite, Io,);
impl_error_code!(
    crate::autonomy::AutonomyError,
    Sqlite,
    Cards,
    Trace,
    Db,
    Json,
);
impl_error_code!(crate::cards::CardsError, Sqlite, Json, Db);
impl_error_code!(crate::commands::RouteError, Trace, Orch);
impl_error_code!(crate::credentials::CredError, Sqlite, Cards);
impl_error_code!(crate::db::DbError, Sqlite);
impl_error_code!(
    crate::install::InstallError,
    Io,
    Sqlite,
    Cards,
    Trace,
    Json,
    Db,
);
impl_error_code!(
    crate::orchestra::OrchError,
    Trace,
    Db,
    Cards,
    Sqlite,
    Json,
    Io,
);
impl_error_code!(
    crate::packedit::PackEditError,
    Validate,
    Io,
    Json,
    Sqlite,
    Db,
    Orch,
);
impl_error_code!(crate::policydev::PolicyDevError, Proposal, Deliver, Db, Io,);
impl_error_code!(crate::presets::PresetError);
impl_error_code!(
    crate::proposals::PropError,
    Sqlite,
    Cards,
    Trace,
    Db,
    Io,
    Git,
);
impl_error_code!(crate::provider::ProviderError);
impl_error_code!(crate::provider_admin::AdminError, Providers, Cred, Json);
impl_error_code!(crate::provider_config::ProvidersError, Io, Json);
impl_error_code!(
    crate::publish::PublishError,
    Sqlite,
    Cards,
    Db,
    Trace,
    Cred,
    Git,
);
impl_error_code!(
    crate::review::ReviewError,
    Artifact,
    Trace,
    Db,
    Cards,
    Orch,
    Sqlite,
);
impl_error_code!(crate::roles::RoleError, Sqlite, Json, Preset, Db, Io,);
impl_error_code!(crate::templates::TemplateError, Io, Json, Preset,);
impl_error_code!(
    crate::setup::SetupError,
    Api,
    Preset,
    Db,
    Cred,
    Sqlite,
    Git,
    Io,
    Autonomy,
    Model,
);
impl_error_code!(
    crate::tools::ToolError,
    Io,
    Trace,
    Db,
    Cards,
    Sqlite,
    Artifact,
);
impl_error_code!(crate::files::RepoFsError, Io);
impl_error_code!(crate::trace::TraceError, Db, Sqlite, Json, Io, Cards,);
impl_error_code!(
    crate::turn::TurnError,
    Tool,
    Trace,
    Provider,
    Sqlite,
    Db,
    Json,
    Orch,
    Cards,
);

/// `git` 壳命令失败是高频可修错误（推送冲突/无远端等），`Cli` 变体名
/// 太笼统——码面固定 `git_cli` 以便 UI 出可执行文案。
impl ErrorCode for crate::git::GitError {
    fn code(&self) -> String {
        match self {
            Self::Io(e) => e.code(),
            Self::Cli(..) => "git_cli".into(),
            _ => variant_code(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_variants_snake_case_debug_head() {
        assert_eq!(
            variant_code(&crate::cards::CardsError::NotFound("q1".into())),
            "not_found"
        );
        assert_eq!(variant_code(&crate::orchestra::OrchError::Paused), "paused");
        // 结构变体、多词变体同样取头词
        assert_eq!(
            variant_code(&crate::cards::CardsError::NotQueued {
                qid: "q".into(),
                state: "answered".into()
            }),
            "not_queued"
        );
        assert_eq!(
            variant_code(&crate::orchestra::OrchError::NoActiveStage("p".into())),
            "no_active_stage"
        );
    }

    #[test]
    fn wrappers_delegate_to_leaf_code() {
        // Api→Cards 包装不停在 "cards"，穿透到叶码——这正是当初没有
        // 选「枚举名前缀」方案的原因：包装层不是用户语义。
        let e = crate::api::ApiError::Cards(crate::cards::CardsError::NotQueued {
            qid: "q".into(),
            state: "answered".into(),
        });
        assert_eq!(e.code(), "not_queued");
        // sqlite 穿多层包装到底
        let e = crate::api::ApiError::Orch(crate::orchestra::OrchError::Sqlite(
            rusqlite::Error::InvalidQuery,
        ));
        assert_eq!(e.code(), "sqlite");
        // 票 09：JudgeError 是叶子模块本地货币，包装后仍穿透到叶码
        let e = crate::api::ApiError::Judge(crate::judge::JudgeError::Cards(
            crate::cards::CardsError::NotFound("q9".into()),
        ));
        assert_eq!(e.code(), "not_found");
    }

    #[test]
    fn git_cli_gets_stable_alias() {
        let e = crate::git::GitError::Cli("push".into(), "rejected".into());
        assert_eq!(e.code(), "git_cli");
    }
}
