//! 疑似凭据判定（owner-followups-1015 #16，ADR 0093 后续裁决「聊天里疑似凭据不转述、不进轨迹，
//! 提示去设置页录入」）：Agent 在聊天里拟登记发布目标时，负责人可能把令牌直接说给它
//! （「用 npm_xxx 发」），模型再把令牌拼进命令行。本判定在拟稿入卡、工具调用入轨迹之前挡住这类文字。
//!
//! 只看文字形状，不看钥匙串：工具上下文拿不到凭据库（测试也不许碰真钥匙串）。已存凭据值的精确比对
//! 由负责人批准时的 `release_chat::approve` 补做（沿用 `release_targets` 的 `release_secret_in_target`）。
//!
//! 判定面（任一命中即「疑似」）：
//! 1. 常见令牌前缀（`ghp_` / `github_pat_` / `glpat-` / `npm_` / `sk-` / `xox?-` / `AKIA` / `pypi-` …）后跟足够长的令牌字符；
//! 2. 机密名的键直接赋字面值：`NPM_TOKEN=abc`、`--password=abc`、`--token abc`、`//registry/:_authToken=abc`——
//!    值是环境变量引用（`$X` / `${X}` / `%X%`）或占位（`<X>`）不算；
//! 3. 长随机串：≥ 32 个令牌字符且大小写字母与数字俱全（纯十六进制不算——提交哈希不是凭据）。
//!
//! 代价模型（fail-closed）：误判（false positive）只花模型一次改写——把值换成凭据名、请负责人去
//! 设置·发布页录入；漏判（false negative）是一枚令牌进卡面、轨迹与登记表，之后每次发布都抄一遍。
//! 偏向判「疑似」。被否决：只比对已存凭据值——负责人刚在聊天里贴出的新令牌还不在钥匙串里，正是要挡的那一类。

/// 令牌前缀与其后至少要跟的令牌字符数。
const PREFIXES: &[(&str, usize)] = &[
    ("ghp_", 16),
    ("gho_", 16),
    ("ghu_", 16),
    ("ghs_", 16),
    ("ghr_", 16),
    ("github_pat_", 16),
    ("glpat-", 16),
    ("npm_", 16),
    ("sk-", 16),
    ("xoxb-", 8),
    ("xoxp-", 8),
    ("xoxa-", 8),
    ("xoxr-", 8),
    ("AKIA", 16),
    ("ASIA", 16),
    ("pypi-", 16),
    ("-----BEGIN", 0),
];

/// 键名里出现即视为机密名（小写比对）。
const SECRET_KEY_WORDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "api-key",
    "private_key",
    "private-key",
    "access_key",
    "access-key",
];

/// 键名以这些结尾说明值是从别处读的（`--password-stdin`、`--token-file`），不是值本身。
const INDIRECT_SUFFIXES: &[&str] = &["stdin", "file", "env", "path"];

/// 长随机串门槛（字符）。
const HIGH_ENTROPY_MIN: usize = 32;

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// 一段文字里是否疑似夹着凭据值。
pub fn suspected(text: &str) -> bool {
    has_prefixed_token(text) || has_secret_assignment(text) || has_high_entropy(text)
}

/// 多个字段任一疑似。
pub fn any_suspected<'a>(fields: impl IntoIterator<Item = &'a str>) -> bool {
    fields.into_iter().any(suspected)
}

fn has_prefixed_token(text: &str) -> bool {
    PREFIXES.iter().any(|(prefix, min)| {
        text.match_indices(prefix).any(|(at, _)| {
            // 前缀前面必须是边界：`mask-1234…` 里的 `sk-` 不是 OpenAI 令牌。
            let boundary = text[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            let run = text[at + prefix.len()..]
                .chars()
                .take_while(|c| token_char(*c))
                .count();
            boundary && run >= *min
        })
    })
}

fn secret_key(key: &str) -> bool {
    let k = key.trim_start_matches('-').to_ascii_lowercase();
    // `//registry.npmjs.org/:_authToken` → `_authtoken`
    let k = k.rsplit([':', '/']).next().unwrap_or_default();
    SECRET_KEY_WORDS.iter().any(|w| k.contains(w))
        && !INDIRECT_SUFFIXES.iter().any(|s| k.ends_with(s))
}

/// 字面值（不是环境变量引用、不是占位、不是另一个开关）。
fn literal(value: &str) -> bool {
    let v = value.trim_matches(|c| c == '"' || c == '\'');
    !v.is_empty() && !v.starts_with(['$', '%', '<', '-'])
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ';' | '|' | '&'))
        .filter(|w| !w.is_empty())
        .collect()
}

fn has_secret_assignment(text: &str) -> bool {
    let words = words(text);
    words.iter().enumerate().any(|(i, word)| {
        if let Some((key, value)) = word.split_once('=') {
            return secret_key(key) && literal(value);
        }
        word.starts_with('-')
            && secret_key(word)
            && words.get(i + 1).is_some_and(|next| literal(next))
    })
}

fn has_high_entropy(text: &str) -> bool {
    text.split(|c: char| !(token_char(c) || c == '+'))
        .any(|seg| {
            seg.len() >= HIGH_ENTROPY_MIN
                && seg.chars().any(|c| c.is_ascii_digit())
                && seg.chars().any(|c| c.is_ascii_lowercase())
                && seg.chars().any(|c| c.is_ascii_uppercase())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn ordinary_release_commands_pass() {
        for cmd in [
            "npm publish",
            "npm publish --access public",
            "cargo publish --token $CARGO_REGISTRY_TOKEN",
            "twine upload dist/* -u __token__ -p ${PYPI_TOKEN}",
            "echo $DOCKER_PASSWORD | docker login --password-stdin -u me",
            "NPM_TOKEN=$NPM_TOKEN npm publish",
            "gh release create v1.2.3 --notes-file CHANGELOG.md",
            "git push origin 3f9a2c4e5b6d7e8f90a1b2c3d4e5f6a7b8c9d0e1",
            "./scripts/deploy.sh --env production --token <TOKEN>",
            "mask-1234567890abcdefghij",
        ] {
            assert!(!suspected(cmd), "{cmd}");
        }
    }

    #[test]
    fn pasted_tokens_are_suspected() {
        for cmd in [
            "npm publish --//registry.npmjs.org/:_authToken=abc123",
            "NPM_TOKEN=abc123 npm publish",
            "cargo publish --token cio_abc",
            "docker login -u me --password hunter2",
            "curl -H 'Authorization: token ghp_0123456789abcdefABCDEF' https://api.github.com",
            "npm publish npm_AbCdEf0123456789xyz",
            "deploy --key sk-proj-abcdefghijklmnop1234",
            "aws s3 cp x s3://b AKIAIOSFODNN7EXAMPLE",
            "upload Zq8XvB3nP0wLk2Rt5YhJ7mC9sDf4GaE1",
        ] {
            assert!(suspected(cmd), "{cmd}");
        }
    }

    /// 不含机密词的普通命令词。
    fn benign_word() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-z][a-z0-9.-]{0,10}",
            "--?[a-z]{1,8}",
            "\\$[A-Z_]{1,10}",
            "[a-z]{1,6}/[a-z]{1,6}",
        ]
        .prop_filter("no secret words", |w| {
            !SECRET_KEY_WORDS
                .iter()
                .any(|s| w.to_ascii_lowercase().contains(s))
        })
    }

    fn benign_command() -> impl Strategy<Value = String> {
        proptest::collection::vec(benign_word(), 1..8).prop_map(|w| w.join(" "))
    }

    proptest! {
        /// 误判上界：只由普通命令词组成的命令永不判疑似。
        #[test]
        fn benign_commands_are_never_suspected(cmd in benign_command()) {
            prop_assert!(!suspected(&cmd), "{}", cmd);
        }

        /// 前缀令牌夹在任何普通命令里都判疑似。
        #[test]
        fn a_prefixed_token_anywhere_is_suspected(
            before in benign_command(),
            after in benign_command(),
            pick in 0..PREFIXES.len() - 1,
            body in "[A-Za-z0-9]{16,40}",
        ) {
            let token = format!("{}{}", PREFIXES[pick].0, body);
            let cmd = format!("{before} {token} {after}");
            prop_assert!(suspected(&cmd), "{}", cmd);
        }

        /// 机密名的键赋字面值判疑似；同一个键引用环境变量则放行。
        #[test]
        fn secret_assignment_literal_vs_reference(
            key in prop::sample::select(vec!["NPM_TOKEN", "--password", "API_KEY", "--token", "CLIENT_SECRET"]),
            value in "[a-z0-9]{1,20}",
            var in "[A-Z_]{1,12}",
            rest in benign_command(),
        ) {
            let literal = format!("{key}={value} {rest}");
            prop_assert!(suspected(&literal), "{}", literal);
            let reference = format!("{key}=${var} {rest}");
            prop_assert!(!suspected(&reference), "{}", reference);
        }

        /// 单调：疑似的文字再拼上普通命令词仍疑似（拼接不能把令牌「洗」掉）。
        #[test]
        fn suspicion_survives_surrounding_text(
            before in benign_command(),
            after in benign_command(),
            secret in "[A-Za-z]{12}[0-9]{4}[A-Z]{8}[a-z]{8}",
        ) {
            prop_assume!(suspected(&secret));
            let cmd = format!("{before} {secret} {after}");
            prop_assert!(suspected(&cmd), "{}", cmd);
        }
    }
}
