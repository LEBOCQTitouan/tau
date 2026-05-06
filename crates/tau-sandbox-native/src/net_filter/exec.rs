//! Command executor abstraction for testability.
//!
//! Real shell-out goes through `RealCommandExecutor`. Unit tests use
//! `MockCommandExecutor` to record calls and return canned output without
//! actually invoking `nft`/`ip`/`nsenter`.

use std::io;
use std::process::Output;

/// Trait for shell-out execution. Allows unit tests to mock subprocess calls.
pub(crate) trait CommandExecutor: Send + Sync {
    /// Run a command with optional stdin.
    ///
    /// Returns the process Output regardless of exit code (caller checks
    /// `.status.success()` and parses stderr).
    fn run(&self, cmd: &str, args: &[&str], stdin: Option<&str>) -> io::Result<Output>;
}

/// Real subprocess executor used in production.
pub(crate) struct RealCommandExecutor;

impl CommandExecutor for RealCommandExecutor {
    fn run(&self, cmd: &str, args: &[&str], stdin: Option<&str>) -> io::Result<Output> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut child = Command::new(cmd)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(input) = stdin {
            if let Some(mut child_stdin) = child.stdin.take() {
                child_stdin.write_all(input.as_bytes())?;
                drop(child_stdin); // EOF
            }
        }

        child.wait_with_output()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::sync::Mutex;

    /// Recorded subprocess invocation.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RecordedCall {
        pub cmd: String,
        pub args: Vec<String>,
        pub stdin: Option<String>,
    }

    /// Canned response a `MockCommandExecutor` returns for the next call.
    #[derive(Debug, Clone)]
    pub(crate) struct CannedOutput {
        pub exit_code: i32,
        pub stdout: Vec<u8>,
        pub stderr: Vec<u8>,
    }

    impl CannedOutput {
        pub fn ok() -> Self {
            Self {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        }
        pub fn err(stderr: impl Into<Vec<u8>>) -> Self {
            Self {
                exit_code: 1,
                stdout: Vec::new(),
                stderr: stderr.into(),
            }
        }
    }

    /// Test-only executor: records calls, returns canned output in FIFO order.
    pub(crate) struct MockCommandExecutor {
        calls: Mutex<Vec<RecordedCall>>,
        responses: Mutex<Vec<CannedOutput>>,
    }

    impl MockCommandExecutor {
        pub fn new(responses: Vec<CannedOutput>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(responses),
            }
        }

        pub fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().expect("mutex").clone()
        }
    }

    impl CommandExecutor for MockCommandExecutor {
        fn run(&self, cmd: &str, args: &[&str], stdin: Option<&str>) -> io::Result<Output> {
            self.calls.lock().expect("mutex").push(RecordedCall {
                cmd: cmd.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                stdin: stdin.map(String::from),
            });
            let canned = self
                .responses
                .lock()
                .expect("mutex")
                .pop()
                .ok_or_else(|| io::Error::other("MockCommandExecutor: no canned response left"))?;
            Ok(Output {
                status: ExitStatus::from_raw(canned.exit_code << 8),
                stdout: canned.stdout,
                stderr: canned.stderr,
            })
        }
    }
}
