//! Subprocess runner with wall-clock timeout + 1 MiB output cap.
//!
//! Implements the kill+drain pattern per spec §5.3, §5.4, §5.5:
//! - Spawn with no stdin, piped stdout/stderr, no env inheritance.
//! - Read stdout + stderr concurrently into Vec<u8> buffers.
//! - tokio::select! between child.wait() and tokio::time::sleep(timeout).
//! - On timeout: child.kill() + child.wait() to reap; partial buffers
//!   are returned with timed_out: true.
//! - Each buffer capped at MAX_OUTPUT_BYTES (1 MiB); excess truncated
//!   and the `*_truncated: bool` flag set.

#![allow(dead_code)] // consumed by Task 13 (plugin.rs invoke).

use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Maximum bytes captured per stream (stdout / stderr).
pub(crate) const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Result of a subprocess invocation.
#[derive(Debug)]
pub(crate) struct RunResult {
    /// Captured stdout bytes (possibly truncated).
    pub stdout: Vec<u8>,
    /// Captured stderr bytes (possibly truncated).
    pub stderr: Vec<u8>,
    /// Process exit code; -1 when the wall-clock timeout fired.
    pub exit_code: i32,
    /// True iff the wall-clock timeout fired (process was SIGKILLed).
    pub timed_out: bool,
    /// True iff stdout exceeded MAX_OUTPUT_BYTES and was truncated.
    pub stdout_truncated: bool,
    /// True iff stderr exceeded MAX_OUTPUT_BYTES and was truncated.
    pub stderr_truncated: bool,
}

/// Run a subprocess with wall-clock timeout + output capping.
///
/// `command` is the program name (looked up via PATH or absolute);
/// `args` are positional arguments. `cwd` is an optional working
/// directory. `timeout_secs` is the wall-clock timeout in seconds.
///
/// On timeout, the child is SIGKILLed and partial buffers are
/// returned with `timed_out: true, exit_code: -1`.
pub(crate) async fn run_subprocess(
    command: &str,
    args: &[String],
    timeout_secs: u64,
    cwd: Option<&str>,
) -> std::io::Result<RunResult> {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .env_clear() // no env inheritance
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn()?;

    // Take stdout/stderr handles before await so we can read them
    // concurrently with child.wait().
    let mut stdout_handle = child.stdout.take().expect("stdout was piped");
    let mut stderr_handle = child.stderr.take().expect("stderr was piped");

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stdout_handle.read_to_end(&mut buf).await;
        buf
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr_handle.read_to_end(&mut buf).await;
        buf
    });

    // tokio::select! between child completion and the timeout.
    let timed_out = tokio::select! {
        _ = child.wait() => false,
        _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
            // Best-effort SIGKILL + reap.
            let _ = child.kill().await;
            let _ = child.wait().await;
            true
        }
    };

    // Join the buffer-collecting tasks. They finish naturally because
    // the child has exited (either normally or via SIGKILL).
    let stdout_raw = stdout_task.await.unwrap_or_default();
    let stderr_raw = stderr_task.await.unwrap_or_default();

    let (stdout, stdout_truncated) = cap_and_flag(stdout_raw);
    let (stderr, stderr_truncated) = cap_and_flag(stderr_raw);

    let exit_code = if timed_out {
        -1
    } else {
        // After child.wait() completed, the child's status was set;
        // try_wait gives us the ExitStatus without re-awaiting.
        match child.try_wait() {
            Ok(Some(status)) => status.code().unwrap_or(-1),
            _ => -1,
        }
    };

    Ok(RunResult {
        stdout,
        stderr,
        exit_code,
        timed_out,
        stdout_truncated,
        stderr_truncated,
    })
}

/// Cap a buffer at MAX_OUTPUT_BYTES; return (capped_buffer, truncated_flag).
pub(crate) fn cap_and_flag(buf: Vec<u8>) -> (Vec<u8>, bool) {
    if buf.len() > MAX_OUTPUT_BYTES {
        (buf[..MAX_OUTPUT_BYTES].to_vec(), true)
    } else {
        (buf, false)
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_echo_returns_stdout() {
        let result = run_subprocess("/bin/echo", &["hello".to_string()], 5, None)
            .await
            .expect("echo runs");
        assert_eq!(result.exit_code, 0);
        assert!(!result.timed_out);
        assert_eq!(result.stdout, b"hello\n");
        assert!(result.stderr.is_empty());
        assert!(!result.stdout_truncated);
        assert!(!result.stderr_truncated);
    }

    #[tokio::test]
    async fn run_nonzero_exit_returns_exit_code() {
        let result = run_subprocess(
            "/bin/sh",
            &["-c".to_string(), "exit 7".to_string()],
            5,
            None,
        )
        .await
        .expect("sh runs");
        assert_eq!(result.exit_code, 7);
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn run_timeout_kills_and_flags_timed_out() {
        let result = run_subprocess(
            "/bin/sh",
            &["-c".to_string(), "sleep 5".to_string()],
            1,
            None,
        )
        .await
        .expect("sh runs");
        assert_eq!(result.exit_code, -1);
        assert!(result.timed_out);
    }

    #[tokio::test]
    async fn run_command_not_found_returns_io_err() {
        let result = run_subprocess("definitely-not-a-real-command-xyz-9999", &[], 5, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_with_cwd_runs_in_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let result = run_subprocess(
            "/bin/sh",
            &["-c".to_string(), "pwd".to_string()],
            5,
            Some(path),
        )
        .await
        .expect("sh runs");
        assert_eq!(result.exit_code, 0);
        // pwd outputs the cwd + newline; canonicalize-tolerant compare.
        let actual = String::from_utf8(result.stdout).unwrap();
        let actual_trimmed = actual.trim();
        let expected_canonical = std::fs::canonicalize(path).unwrap();
        let actual_canonical = std::fs::canonicalize(actual_trimmed).unwrap();
        assert_eq!(actual_canonical, expected_canonical);
    }

    #[test]
    fn cap_and_flag_under_limit_no_flag() {
        let buf = vec![b'a'; 100];
        let (capped, truncated) = cap_and_flag(buf);
        assert_eq!(capped.len(), 100);
        assert!(!truncated);
    }

    #[test]
    fn cap_and_flag_at_exact_limit_no_flag() {
        let buf = vec![b'a'; MAX_OUTPUT_BYTES];
        let (capped, truncated) = cap_and_flag(buf);
        assert_eq!(capped.len(), MAX_OUTPUT_BYTES);
        assert!(!truncated);
    }

    #[test]
    fn cap_and_flag_over_limit_truncates() {
        let buf = vec![b'a'; MAX_OUTPUT_BYTES + 1024];
        let (capped, truncated) = cap_and_flag(buf);
        assert_eq!(capped.len(), MAX_OUTPUT_BYTES);
        assert!(truncated);
    }
}
