//! Controlled-environment test binary for sub-project B's landlock
//! e2e tests, extended in sub-project D for seccomp + exec coverage.
//!
//! # Mode dispatch
//!
//! `TAU_FIXTURE_MODE` env var selects the binary's behavior:
//!
//! - `read` (or unset, when `TAU_FIXTURE_INPUT_PATH` is set):
//!   Read up to 256 bytes from `TAU_FIXTURE_INPUT_PATH`, emit
//!   `READ_OK <bytes>\n` to stdout, exit 0.
//! - `open-socket`: call `socket(AF_INET, SOCK_STREAM, 0)`. On success,
//!   emit `SOCKET_OK\n` and exit 0. On EACCES / EPERM, emit error and
//!   exit 1. SIGSYS from seccomp → no output, signal exit.
//! - `exec`: spawn `${TAU_FIXTURE_EXEC_CMD}` with no args, proxy its
//!   stdout, exit with its exit code.
//! - `default` (or no env vars set): emit `CONTROLLED_ENV_OK\n`,
//!   exit 0.
//!
//! Statically-linked release builds avoid landlock false positives on
//! Ubuntu CI's `/bin → /usr/bin` symlink layout.

use std::io::{Read, Write};

fn main() {
    let mode = std::env::var("TAU_FIXTURE_MODE").ok();
    let path = std::env::var("TAU_FIXTURE_INPUT_PATH").ok();

    let result = match mode.as_deref() {
        Some("read") => match path {
            Some(p) => read_and_emit(&p),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "TAU_FIXTURE_MODE=read requires TAU_FIXTURE_INPUT_PATH",
            )),
        },
        Some("open-socket") => open_socket(),
        Some("exec") => exec_proxy(),
        Some("default") | None => match path {
            Some(p) => read_and_emit(&p),
            None => emit_default(),
        },
        Some(other) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown TAU_FIXTURE_MODE: {other}"),
        )),
    };

    if let Err(e) = result {
        eprintln!("controlled-env-binary error: {e}");
        std::process::exit(1);
    }
}

fn read_and_emit(path: &str) -> std::io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 256];
    let n = file.read(&mut buf)?;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(b"READ_OK ")?;
    handle.write_all(&buf[..n])?;
    handle.write_all(b"\n")?;
    handle.flush()?;
    Ok(())
}

fn emit_default() -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(b"CONTROLLED_ENV_OK\n")?;
    handle.flush()?;
    Ok(())
}

fn open_socket() -> std::io::Result<()> {
    // Use libc directly to avoid pulling std::net which may add dynamic deps.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        let err = std::io::Error::last_os_error();
        return Err(err);
    }
    unsafe { libc::close(fd) };
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(b"SOCKET_OK\n")?;
    handle.flush()?;
    Ok(())
}

fn exec_proxy() -> std::io::Result<()> {
    let cmd = std::env::var("TAU_FIXTURE_EXEC_CMD").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "TAU_FIXTURE_MODE=exec requires TAU_FIXTURE_EXEC_CMD",
        )
    })?;
    let output = std::process::Command::new(&cmd).output()?;
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stdout().flush()?;
    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}
