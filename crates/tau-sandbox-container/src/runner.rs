//! Build the `docker run` / `podman run` argv from a [`SandboxPlan`].
//!
//! The public-facing entry point is [`wrap_command`], which rewrites a
//! [`std::process::Command`] in-place. [`build_run_args`] is extracted as a
//! pure function so it can be unit-tested without spawning anything.

use std::process::Command;

use tau_domain::{Capability, FsCapability, NetCapability};
use tau_ports::{SandboxError, SandboxHandle, SandboxPlan};

use crate::probe::ResolvedRuntime;

/// Default base image used when no per-plugin override is configured.
///
/// TODO(task-7): callers should pass the image via SandboxPlan or scope
/// config; this constant becomes the documented v0.1 fallback.
const DEFAULT_BASE_IMAGE: &str = "ghcr.io/tau-runtime/sandbox-base:v0.1";

/// Replace `cmd` with a `docker run` (or `podman run`) invocation that wraps
/// the original program and args inside a container.
///
/// The original [`Command`]'s program and arguments are extracted, then `*cmd`
/// is replaced with a new `Command` whose argv is the container runtime
/// invocation.
///
/// Returns [`SandboxHandle::noop`]: container teardown is handled by
/// `--rm`; no parent-side cleanup is needed.
///
/// ## Caller config
///
/// - **stdio**: set to `Stdio::piped()` for stdin, stdout, and stderr after
///   `wrap_command` returns. The original caller's stdio configuration is
///   dropped (stable `std::process::Command` does not expose stdio inspection).
///   If different stdio is needed, reconfigure via the returned `Command`.
///
/// - **env vars**: env vars whose names match `TAU_*` or `RUST_LOG` are
///   captured from the original `cmd` and forwarded into the container via
///   `-e KEY=VALUE` flags. All other env vars are dropped — the container
///   image provides its own environment.
///
/// - **cwd**: dropped — has no meaning inside a container with `--read-only`
///   root and `--tmpfs /tmp`.
pub(crate) fn wrap_command(
    plan: &SandboxPlan,
    cmd: &mut Command,
    runtime: ResolvedRuntime,
) -> Result<SandboxHandle, SandboxError> {
    let original_program = cmd.get_program().to_string_lossy().into_owned();
    let original_args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    // Capture env vars matching TAU_* or RUST_LOG before we replace cmd.
    let forwarded_envs: Vec<(String, String)> = cmd
        .get_envs()
        .filter_map(|(k, v)| {
            let key = k.to_string_lossy().to_string();
            let val = v?.to_string_lossy().to_string();
            if key.starts_with("TAU_") || key == "RUST_LOG" {
                Some((key, val))
            } else {
                None
            }
        })
        .collect();

    let argv = build_run_args(
        plan,
        runtime,
        &original_program,
        &original_args,
        &forwarded_envs,
    );

    // Replace cmd in-place: clear via re-assignment, then build new argv.
    *cmd = Command::new(runtime.binary());
    for arg in &argv {
        cmd.arg(arg);
    }
    // Set stdio to piped so the container's stdin/stdout/stderr are accessible
    // to the caller (required for plugin host JSON-RPC IPC in Task 9).
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    Ok(SandboxHandle::noop())
}

/// Build the argv slice (without the `docker`/`podman` binary itself) that
/// wraps `program` and `program_args` inside a container derived from `plan`.
///
/// `forwarded_envs` is a list of `(KEY, VALUE)` pairs that will be injected
/// into the container via `-e KEY=VALUE` flags. Typically these are env vars
/// matching `TAU_*` or `RUST_LOG` captured from the original `Command`.
///
/// Exposed for unit tests so argv shape can be verified without spawning.
pub(crate) fn build_run_args(
    plan: &SandboxPlan,
    runtime: ResolvedRuntime,
    program: &str,
    program_args: &[String],
    forwarded_envs: &[(String, String)],
) -> Vec<String> {
    // Both docker and podman share the same `run` argument shape.
    let _ = runtime;

    let mut argv: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-i".into(),
        "--user".into(),
        "nobody".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        "--read-only".into(),
        "--pids-limit".into(),
        "256".into(),
        "--ipc=none".into(),
        "--tmpfs".into(),
        "/tmp:size=64m".into(),
    ];

    // Network: bridge only when the plan requests HTTP; otherwise none.
    let has_http = plan
        .capabilities
        .iter()
        .any(|c| matches!(c, Capability::Network(NetCapability::Http { .. })));
    argv.push("--network".into());
    if has_http {
        argv.push("bridge".into());
    } else {
        argv.push("none".into());
    }

    // Volume mounts derived from filesystem capabilities.
    for cap in &plan.capabilities {
        match cap {
            Capability::Filesystem(FsCapability::Read { paths, .. }) => {
                for p in paths {
                    let cleaned = clean_mount_path(p);
                    argv.push("-v".into());
                    argv.push(format!("{cleaned}:{cleaned}:ro"));
                }
            }
            Capability::Filesystem(FsCapability::Write { paths, .. }) => {
                for p in paths {
                    let cleaned = clean_mount_path(p);
                    argv.push("-v".into());
                    argv.push(format!("{cleaned}:{cleaned}:rw"));
                }
            }
            // ProcessExec and Network are handled via --cap-drop / --network;
            // no extra mount args needed.
            _ => {}
        }
    }

    // Forward whitelisted env vars into the container.
    for (k, v) in forwarded_envs {
        argv.push("-e".into());
        argv.push(format!("{k}={v}"));
    }

    // Image and original command come last.
    argv.push(DEFAULT_BASE_IMAGE.into());
    argv.push(program.into());
    for a in program_args {
        argv.push(a.clone());
    }

    argv
}

/// Strip trailing glob suffixes from a path so it can be used as a bind-mount
/// source/target. For example `/srv/data/**` becomes `/srv/data`.
fn clean_mount_path(p: &str) -> String {
    p.trim_end_matches("/**")
        .trim_end_matches("/*")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan_from(capabilities: serde_json::Value) -> SandboxPlan {
        let plan_json = json!({
            "capabilities": capabilities,
            "context": null,
            "limits": null,
        });
        serde_json::from_value(plan_json).expect("decode plan")
    }

    #[test]
    fn read_path_yields_ro_mount() {
        let plan = plan_from(json!([{
            "kind": "fs.read",
            "paths": ["/etc/foo"]
        }]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(
            argv.iter().any(|a| a == "/etc/foo:/etc/foo:ro"),
            "expected ro mount, got {argv:?}"
        );
    }

    #[test]
    fn write_path_yields_rw_mount() {
        let plan = plan_from(json!([{
            "kind": "fs.write",
            "paths": ["/data/cache"]
        }]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(
            argv.iter().any(|a| a == "/data/cache:/data/cache:rw"),
            "expected rw mount, got {argv:?}"
        );
    }

    #[test]
    fn http_capability_enables_bridge_network() {
        let plan = plan_from(json!([{
            "kind": "net.http",
            "hosts": ["api.example.com"],
            "methods": ["GET"]
        }]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        let pos = argv
            .iter()
            .position(|a| a == "--network")
            .expect("--network present");
        assert_eq!(argv[pos + 1], "bridge");
    }

    #[test]
    fn no_network_capability_uses_none() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        let pos = argv
            .iter()
            .position(|a| a == "--network")
            .expect("--network present");
        assert_eq!(argv[pos + 1], "none");
    }

    #[test]
    fn read_only_root_and_tmpfs_present() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(
            argv.iter().any(|a| a == "--read-only"),
            "missing --read-only"
        );
        assert!(argv.iter().any(|a| a == "--tmpfs"), "missing --tmpfs");
        assert!(
            argv.iter().any(|a| a == "/tmp:size=64m"),
            "missing /tmp:size=64m"
        );
    }

    #[test]
    fn cap_drop_and_no_new_privs_present() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(
            argv.iter().any(|a| a == "--cap-drop=ALL"),
            "missing --cap-drop=ALL"
        );
        assert!(
            argv.iter().any(|a| a == "--security-opt=no-new-privileges"),
            "missing --security-opt=no-new-privileges"
        );
    }

    #[test]
    fn hardening_flags_present() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        let user_pos = argv
            .iter()
            .position(|a| a == "--user")
            .expect("--user present");
        assert_eq!(argv[user_pos + 1], "nobody", "--user must be nobody");
        let pids_pos = argv
            .iter()
            .position(|a| a == "--pids-limit")
            .expect("--pids-limit present");
        assert_eq!(argv[pids_pos + 1], "256", "--pids-limit must be 256");
        assert!(argv.iter().any(|a| a == "--ipc=none"), "missing --ipc=none");
    }

    #[test]
    fn original_program_and_args_at_end() {
        let plan = plan_from(json!([]));
        let args: Vec<String> = vec!["hello".into(), "world".into()];
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &args, &[]);
        assert_eq!(argv[argv.len() - 3], "/bin/echo");
        assert_eq!(argv[argv.len() - 2], "hello");
        assert_eq!(argv[argv.len() - 1], "world");
    }

    #[test]
    fn glob_suffix_trimmed_in_mounts() {
        let plan = plan_from(json!([{
            "kind": "fs.read",
            "paths": ["/srv/data/**"]
        }]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(
            argv.iter().any(|a| a == "/srv/data:/srv/data:ro"),
            "expected globless mount, got {argv:?}"
        );
    }

    #[test]
    fn podman_runtime_produces_same_argv_shape() {
        // Both runtimes should produce identical argv (the binary is set
        // outside build_run_args).
        let plan = plan_from(json!([]));
        let docker_argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/true", &[], &[]);
        let podman_argv = build_run_args(&plan, ResolvedRuntime::Podman, "/bin/true", &[], &[]);
        assert_eq!(docker_argv, podman_argv);
    }

    #[test]
    fn multiple_read_paths_yield_multiple_mounts() {
        let plan = plan_from(json!([{
            "kind": "fs.read",
            "paths": ["/a", "/b", "/c"]
        }]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(argv.iter().any(|a| a == "/a:/a:ro"));
        assert!(argv.iter().any(|a| a == "/b:/b:ro"));
        assert!(argv.iter().any(|a| a == "/c:/c:ro"));
    }

    #[test]
    fn image_default_is_present_before_program() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        let image_pos = argv
            .iter()
            .position(|a| a == DEFAULT_BASE_IMAGE)
            .expect("default image present");
        let prog_pos = argv
            .iter()
            .position(|a| a == "/bin/echo")
            .expect("program present");
        assert!(
            image_pos < prog_pos,
            "image must appear before program: {argv:?}"
        );
    }

    #[test]
    fn rm_and_stdin_flags_present() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(argv.iter().any(|a| a == "--rm"), "missing --rm");
        assert!(argv.iter().any(|a| a == "-i"), "missing -i");
    }

    #[test]
    fn tau_env_vars_forwarded() {
        let plan = plan_from(json!([]));
        let envs = vec![("TAU_FOO".to_string(), "bar".to_string())];
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &envs);
        let pos = argv
            .iter()
            .position(|a| a == "-e")
            .expect("-e flag present for TAU_FOO");
        assert_eq!(argv[pos + 1], "TAU_FOO=bar", "env must be TAU_FOO=bar");
    }

    #[test]
    fn unrelated_env_vars_dropped() {
        let plan = plan_from(json!([]));
        // LANG is not TAU_* and not RUST_LOG — it must not be forwarded.
        // build_run_args only injects what is passed in forwarded_envs.
        // wrap_command filters; here we verify the contract: if we pass no
        // forwarded envs, no -e flags appear.
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[]);
        assert!(
            !argv.iter().any(|a| a.starts_with("LANG=")),
            "LANG must not appear in argv: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "-e"),
            "no -e flag expected when forwarded_envs is empty: {argv:?}"
        );
    }

    #[test]
    fn rust_log_env_var_forwarded() {
        let plan = plan_from(json!([]));
        let envs = vec![("RUST_LOG".to_string(), "trace".to_string())];
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &envs);
        let pos = argv
            .iter()
            .position(|a| a == "-e")
            .expect("-e flag present for RUST_LOG");
        assert_eq!(argv[pos + 1], "RUST_LOG=trace");
    }
}
