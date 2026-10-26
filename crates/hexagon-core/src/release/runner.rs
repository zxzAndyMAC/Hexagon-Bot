//! 发布命令的子进程执行与输出脱敏（票 34 自 `release.rs` 拆出，行为不变）：跑命令、分路捕获
//! stdout / stderr、超时整组杀、按凭据值脱敏。前置闸、认领与落轨迹留在父模块。

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use super::{ReleaseRunError, REDACTED};

/// 每路输出在内存里最多留的字节（只留最新的）；远大于末尾窗口，截断边界落不进末尾。
const CAPTURE_BYTES: usize = 1 << 20;

/// 把每个凭据值换成 [`REDACTED`]。长的先换，免得一个值是另一个值的子串时留下半截。
pub(super) fn redact(output: &str, secrets: &[(String, String)]) -> String {
    let mut values: Vec<&str> = secrets
        .iter()
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values
        .into_iter()
        .fold(output.to_string(), |acc, v| acc.replace(v, REDACTED))
}

/// 一路子进程输出管道及其独立缓冲（stdout / stderr 各一，S13 分缓冲各自脱敏）。
type Pipe = (Box<dyn Read + Send>, Arc<Mutex<Stream>>);

/// 一路输出（stdout 或 stderr）的捕获：只留最新的 [`CAPTURE_BYTES`]，`truncated` = 开头被丢过。
#[derive(Default)]
pub(super) struct Stream {
    bytes: Vec<u8>,
    truncated: bool,
}

/// 一路输出脱敏：截断过就先丢掉第一行残段（截断点可能落在凭据值中间，留下的后半截
/// [`redact`] 认不出），再整段替换凭据值。
fn clean_stream(s: &Stream, secrets: &[(String, String)]) -> String {
    let text = String::from_utf8_lossy(&s.bytes);
    let kept = if s.truncated {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    redact(kept, secrets)
}

/// 轨迹里的输出 = stdout 脱敏 + stderr 脱敏（各自成段，stdout 在前）。
/// ticket 33（2026-10-25 批次审查 S13，原票 #15）修复即文档：两路曾合流进同一个缓冲再脱敏——
/// 读线程按块交错写入，stdout 的一个凭据值中间可以插进一块 stderr，合流后的文本里凭据不再连续，
/// `redact` 认不出，两半各自原样进了轨迹；截断点落在凭据值中间时同理。被否决：合流后按行脱敏
/// （交错不按行发生，一行里照样能插进另一路的字节）；保留交错顺序（顺序对排错有用，但值不能为它冒泄漏）。
pub(super) fn assemble(r: &Run, secrets: &[(String, String)]) -> String {
    let out = clean_stream(&r.stdout, secrets);
    let err = clean_stream(&r.stderr, secrets);
    match (out.is_empty(), err.is_empty()) {
        (_, true) => out,
        (true, false) => err,
        (false, false) if out.ends_with('\n') => out + &err,
        (false, false) => out + "\n" + &err,
    }
}

pub(super) fn tail(s: &str, chars: usize) -> String {
    let n = s.chars().count();
    if n <= chars {
        return s.to_string();
    }
    s.chars().skip(n - chars).collect()
}

pub(super) enum RunStatus {
    /// 退出码；被信号终止为 None。
    Exited(Option<i32>),
    TimedOut,
}

pub(super) struct Run {
    pub(super) status: RunStatus,
    pub(super) stdout: Stream,
    pub(super) stderr: Stream,
}

/// 跑一条登记的命令行：unix `sh -c`（自成进程组，超时整组杀），Windows `cmd /C`。
/// stdin 关闭（发布命令不能停下来等输入，等就是卡死）；stdout / stderr 各自一个缓冲、各留最新的
/// [`CAPTURE_BYTES`]（分开脱敏，见 [`assemble`]）。凭据作为环境变量交给子进程。
pub(super) fn run_command(
    command: &str,
    dir: &Path,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<Run, ReleaseRunError> {
    let mut c = shell_command(command, dir, env);
    // 只记错误种类：spawn 错误文案可能带命令片段。
    let mut child = c
        .spawn()
        .map_err(|e| ReleaseRunError::ReleaseSpawn(format!("{:?}", e.kind())))?;
    let out_buf = Arc::new(Mutex::new(Stream::default()));
    let err_buf = Arc::new(Mutex::new(Stream::default()));
    let pipes: Vec<Pipe> = [
        child
            .stdout
            .take()
            .map(|p| (Box::new(p) as Box<dyn Read + Send>, out_buf.clone())),
        child
            .stderr
            .take()
            .map(|p| (Box::new(p) as Box<dyn Read + Send>, err_buf.clone())),
    ]
    .into_iter()
    .flatten()
    .collect();
    let readers = pipes.len();
    let done_rx = spawn_readers(pipes);
    let status = wait_with_deadline(&mut child, timeout)?;
    await_readers(&done_rx, readers);
    let take =
        |b: &Arc<Mutex<Stream>>| std::mem::take(&mut *b.lock().unwrap_or_else(|p| p.into_inner()));
    Ok(Run {
        status,
        stdout: take(&out_buf),
        stderr: take(&err_buf),
    })
}

/// 组装子进程：平台 shell、工作目录、stdin 关闭、两路输出管道、凭据环境变量。
fn shell_command(command: &str, dir: &Path, env: &[(String, String)]) -> Command {
    #[cfg(unix)]
    let mut c = {
        use std::os::unix::process::CommandExt;
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(command).process_group(0);
        c
    };
    #[cfg(not(unix))]
    let mut c = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    c.current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in env {
        c.env(name, value);
    }
    c
}

/// 每路管道一个读线程，写进各自的缓冲（只留最新的 [`CAPTURE_BYTES`]）；读完各发一次完成信号。
fn spawn_readers(pipes: Vec<Pipe>) -> mpsc::Receiver<()> {
    let (done_tx, done_rx) = mpsc::channel::<()>();
    for (mut pipe, buf) in pipes {
        let done = done_tx.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut b = buf.lock().unwrap_or_else(|p| p.into_inner());
                b.bytes.extend_from_slice(&chunk[..n]);
                if b.bytes.len() > CAPTURE_BYTES {
                    let cut = b.bytes.len() - CAPTURE_BYTES;
                    b.bytes.drain(..cut);
                    b.truncated = true;
                }
            }
            let _ = done.send(());
        });
    }
    done_rx
}

/// 轮询等子进程退出；到墙钟上限就整组杀掉并收尸。
fn wait_with_deadline(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<RunStatus, ReleaseRunError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| ReleaseRunError::ReleaseSpawn(format!("{:?}", e.kind())))?
        {
            return Ok(RunStatus::Exited(status.code()));
        }
        if Instant::now() >= deadline {
            kill_tree(child);
            let _ = child.wait();
            return Ok(RunStatus::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 命令退出后，留在后台的孙进程可能还握着管道：最多再等 2 秒收尾，不无限等。
fn await_readers(done_rx: &mpsc::Receiver<()>, readers: usize) {
    let grace = Instant::now() + Duration::from_secs(2);
    for _ in 0..readers {
        let left = grace.saturating_duration_since(Instant::now());
        if done_rx.recv_timeout(left).is_err() {
            break;
        }
    }
}

fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // process_group(0)：组号 = 子进程 pid；负号 = 整组（含 sh 派生的孙进程）。
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: kill(2) 只发信号，不触碰本进程内存。
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn redact_replaces_longest_first() {
        let secrets = vec![
            ("A".to_string(), "tok".to_string()),
            ("B".to_string(), "token-123".to_string()),
        ];
        assert_eq!(
            redact("x token-123 y tok", &secrets),
            format!("x {REDACTED} y {REDACTED}")
        );
    }

    fn stream(text: &str, truncated: bool) -> Stream {
        Stream {
            bytes: text.as_bytes().to_vec(),
            truncated,
        }
    }

    #[test]
    fn truncated_stream_drops_the_partial_first_line() {
        // ticket 33 S13：截断点落在凭据值中间，留下的后半截不得进轨迹。
        let secrets = vec![("TOKEN".to_string(), "token-123".to_string())];
        let s = stream("en-123 tail\nnext token-123\n", true);
        assert_eq!(clean_stream(&s, &secrets), format!("next {REDACTED}\n"));
        assert_eq!(
            clean_stream(&stream("en-123 no newline", true), &secrets),
            ""
        );
        assert_eq!(
            clean_stream(&stream("en-123\n", false), &secrets),
            "en-123\n",
            "untruncated output is kept as is"
        );
    }

    #[cfg(unix)]
    #[test]
    fn secret_split_by_interleaved_stderr_is_still_redacted() {
        // ticket 33（2026-10-25 批次审查 S13）回归：stdout 写凭据前半 → stderr 插一块 → stdout 写后半。
        // 合流缓冲时轨迹里是「tok<stderr>en-123」，两半都漏；分路后 stdout 连续、整值替换。
        let dir = tempfile::tempdir().unwrap();
        let secrets = vec![("TOKEN".to_string(), "token-123".to_string())];
        let run = run_command(
            "printf tok; sleep 0.2; printf X >&2; sleep 0.2; printf 'en-123\\n'",
            dir.path(),
            &secrets,
            Duration::from_secs(10),
        )
        .unwrap();
        let out = assemble(&run, &secrets);
        assert_eq!(out, format!("{REDACTED}\nX"));
        assert!(!out.contains("tok") && !out.contains("en-123"), "{out}");
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
    }

    proptest! {
        /// 脱敏不变量：任何凭据值都不会留在脱敏后的输出里，不管它出现几次、夹在什么文字中间。
        #[test]
        fn redacted_output_never_contains_a_secret(
            secret in "[A-Za-z0-9_-]{4,24}",
            parts in proptest::collection::vec("[ -~]{0,16}", 0..6),
        ) {
            let output = parts.join(&secret);
            let cleaned = redact(&output, &[("TOKEN".into(), secret.clone())]);
            prop_assert!(!cleaned.contains(&secret) || REDACTED.contains(&secret));
        }

        /// ticket 33 S13：stdout / stderr 分路后，任一路里的凭据值（无论被拆成几次写入）都不留在合成输出里。
        #[test]
        fn assembled_output_never_contains_a_secret(
            secret in "[A-Za-z0-9_-]{4,24}",
            out_parts in proptest::collection::vec("[ -~]{0,16}", 0..4),
            err_parts in proptest::collection::vec("[ -~]{0,16}", 0..4),
            truncated in any::<bool>(),
        ) {
            let run = Run {
                status: RunStatus::Exited(Some(0)),
                stdout: stream(&out_parts.join(&secret), truncated),
                stderr: stream(&err_parts.join(&secret), false),
            };
            let cleaned = assemble(&run, &[("TOKEN".into(), secret.clone())]);
            prop_assert!(!cleaned.contains(&secret) || REDACTED.contains(&secret));
        }
    }
}
