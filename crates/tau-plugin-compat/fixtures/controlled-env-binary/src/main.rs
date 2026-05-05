//! Controlled-environment test binary for sub-project B's landlock
//! e2e tests.
//!
//! Designed to perform predictable, side-effect-free I/O so landlock V1
//! path resolution can be exercised without `/bin → /usr/bin` symlink
//! quirks or dynamic linker probing surprises.
//!
//! # Behavior
//!
//! 1. Reads `${TAU_FIXTURE_INPUT_PATH}` env var. If set, attempts to
//!    read that file and writes the first 256 bytes (or fewer if smaller)
//!    to stdout, prefixed with `READ_OK ` and a trailing newline.
//! 2. If `${TAU_FIXTURE_INPUT_PATH}` is not set, writes
//!    `CONTROLLED_ENV_OK\n` to stdout.
//! 3. Exits 0 on success; exits non-zero with a diagnostic on stderr
//!    on any I/O error.
//!
//! Statically-linked release builds avoid landlock false positives on
//! Ubuntu CI's `/bin → /usr/bin` symlink layout.

use std::io::{Read, Write};

fn main() {
    let path = std::env::var("TAU_FIXTURE_INPUT_PATH").ok();

    let result = match path {
        Some(p) => read_and_emit(&p),
        None => emit_default(),
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
