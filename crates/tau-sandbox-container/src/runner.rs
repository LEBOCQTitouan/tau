//! Build the `docker run` / `podman run` argv from a [`SandboxPlan`].
//!
//! The public-facing entry point is [`wrap_command`], which rewrites a
//! [`std::process::Command`] in-place. [`build_run_args`] is extracted as a
//! pure function so it can be unit-tested without spawning anything.

use std::path::PathBuf;
use std::process::Command;

use tau_domain::{Capability, FsCapability, NetCapability};
use tau_ports::{SandboxError, SandboxHandle, SandboxPlan};

use crate::probe::ResolvedRuntime;

/// Proxy configuration for Network(Http) plans.
///
/// Holds the Unix socket path the proxy is listening on and the path to the
/// `tau-net-bridge` binary that will be bind-mounted into the container.
#[derive(Debug)]
pub(crate) struct ProxyConfig {
    /// Absolute path to the proxy Unix socket in the parent's temp dir.
    pub(crate) sock_path: PathBuf,
    /// Path to the `tau-net-bridge` binary that will be bind-mounted into the
    /// container at `/usr/local/bin/tau-net-bridge`.
    pub(crate) bridge_path: String,
}

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
/// For plans with [`tau_domain::NetCapability::Http`], spawns a userspace proxy
/// task and bind-mounts it into the container via a Unix socket, along with the
/// `tau-net-bridge` binary. The proxy guard is nested into the returned
/// [`SandboxHandle`] for LIFO cleanup. The bridge binary path is read from the
/// `TAU_NET_BRIDGE_PATH` environment variable (default: `"tau-net-bridge"`,
/// resolved via `PATH`).
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

    // Determine if the plan requests outbound HTTP.
    let has_network_http = plan
        .capabilities
        .iter()
        .any(|c| matches!(c, Capability::Network(NetCapability::Http { .. })));

    // For Network(Http): spawn the userspace proxy, collect proxy config.
    // Proxy support is unix-only (relies on Unix-domain sockets + the
    // tau-net-bridge Linux binary). On non-unix targets, Network(Http) plans
    // are hard-refused — Windows/etc. container support is a future iteration.
    #[cfg(unix)]
    let (proxy_handle, proxy_config) = if has_network_http {
        // Collect allowed hosts from all Http capabilities.
        let mut allowed_hosts: Vec<String> = Vec::new();
        for cap in &plan.capabilities {
            if let Capability::Network(NetCapability::Http { hosts, .. }) = cap {
                allowed_hosts.extend(hosts.iter().cloned());
            }
        }

        // Validate hosts: rejects wildcards + non-loopback IP literals.
        tau_sandbox_proxy::validate_hosts(&allowed_hosts).map_err(|e| SandboxError::Proxy {
            message: format!("host validation: {e}"),
        })?;

        // Spawn the proxy task in the parent's tokio runtime.
        let handle =
            tau_sandbox_proxy::spawn_proxy(allowed_hosts).map_err(|e| SandboxError::Proxy {
                message: format!("spawn_proxy: {e}"),
            })?;
        let sock_path = handle.sock_path().to_path_buf();

        // Resolve bridge binary path: runtime env var, default to PATH lookup.
        let bridge_path =
            std::env::var("TAU_NET_BRIDGE_PATH").unwrap_or_else(|_| "tau-net-bridge".to_string());

        let config = ProxyConfig {
            sock_path,
            bridge_path,
        };

        (Some(handle), Some(config))
    } else {
        (None, None)
    };

    #[cfg(not(unix))]
    let (proxy_handle, proxy_config): (Option<()>, Option<ProxyConfig>) = if has_network_http {
        return Err(SandboxError::Proxy {
            message: "Network(Http) capability is unix-only in this iteration; \
                      Windows container proxy is a future sub-project"
                .to_string(),
        });
    } else {
        (None, None)
    };

    let argv = build_run_args(
        plan,
        runtime,
        &original_program,
        &original_args,
        &forwarded_envs,
        proxy_config.as_ref(),
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

    // Nest the proxy guard inside the SandboxHandle so it is dropped (LIFO)
    // when the handle is dropped.
    let mut handle = SandboxHandle::noop();
    if let Some(p) = proxy_handle {
        handle.nest_handle(Box::new(p));
    }

    Ok(handle)
}

/// Build the argv slice (without the `docker`/`podman` binary itself) that
/// wraps `program` and `program_args` inside a container derived from `plan`.
///
/// `forwarded_envs` is a list of `(KEY, VALUE)` pairs that will be injected
/// into the container via `-e KEY=VALUE` flags. Typically these are env vars
/// matching `TAU_*` or `RUST_LOG` captured from the original `Command`.
///
/// `proxy` is `Some` when the plan has `Network(Http)` capabilities. When
/// present, the proxy Unix socket is bind-mounted into the container at
/// `/run/tau-proxy.sock:ro`, the bridge binary is bind-mounted at
/// `/usr/local/bin/tau-net-bridge:ro`, and `HTTPS_PROXY=http://127.0.0.1:8443`
/// is injected. The original program is also wrapped with `tau-net-bridge`.
///
/// Exposed for unit tests so argv shape can be verified without spawning.
pub(crate) fn build_run_args(
    plan: &SandboxPlan,
    runtime: ResolvedRuntime,
    program: &str,
    program_args: &[String],
    forwarded_envs: &[(String, String)],
    proxy: Option<&ProxyConfig>,
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

    // Proxy bind-mounts and env injection for Network(Http) plans.
    if let Some(proxy_cfg) = proxy {
        let sock = proxy_cfg.sock_path.display().to_string();
        // Bind-mount proxy socket (read-write so the plugin can connect to it).
        argv.push("-v".into());
        argv.push(format!("{sock}:/run/tau-proxy.sock:rw"));
        // Bind-mount the bridge binary (read-only).
        argv.push("-v".into());
        argv.push(format!(
            "{}:/usr/local/bin/tau-net-bridge:ro",
            proxy_cfg.bridge_path
        ));
        // Inject HTTPS_PROXY so standard HTTP clients inside the container
        // route through tau-net-bridge → proxy.
        argv.push("-e".into());
        argv.push("HTTPS_PROXY=http://127.0.0.1:8443".into());
    }

    // Forward whitelisted env vars into the container.
    for (k, v) in forwarded_envs {
        argv.push("-e".into());
        argv.push(format!("{k}={v}"));
    }

    // Image and original command come last.
    // When a proxy is active, wrap with tau-net-bridge so outbound HTTPS
    // routes through the proxy socket.
    argv.push(DEFAULT_BASE_IMAGE.into());
    if proxy.is_some() {
        argv.push("/usr/local/bin/tau-net-bridge".into());
        argv.push("--proxy-sock=/run/tau-proxy.sock".into());
        argv.push("--listen=127.0.0.1:8443".into());
        argv.push("--".into());
    }
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
        let pos = argv
            .iter()
            .position(|a| a == "--network")
            .expect("--network present");
        assert_eq!(argv[pos + 1], "bridge");
    }

    #[test]
    fn no_network_capability_uses_none() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
        let pos = argv
            .iter()
            .position(|a| a == "--network")
            .expect("--network present");
        assert_eq!(argv[pos + 1], "none");
    }

    #[test]
    fn read_only_root_and_tmpfs_present() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let argv = build_run_args(
            &plan,
            ResolvedRuntime::Docker,
            "/bin/echo",
            &args,
            &[],
            None,
        );
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let docker_argv =
            build_run_args(&plan, ResolvedRuntime::Docker, "/bin/true", &[], &[], None);
        let podman_argv =
            build_run_args(&plan, ResolvedRuntime::Podman, "/bin/true", &[], &[], None);
        assert_eq!(docker_argv, podman_argv);
    }

    #[test]
    fn multiple_read_paths_yield_multiple_mounts() {
        let plan = plan_from(json!([{
            "kind": "fs.read",
            "paths": ["/a", "/b", "/c"]
        }]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
        assert!(argv.iter().any(|a| a == "/a:/a:ro"));
        assert!(argv.iter().any(|a| a == "/b:/b:ro"));
        assert!(argv.iter().any(|a| a == "/c:/c:ro"));
    }

    #[test]
    fn image_default_is_present_before_program() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
        assert!(argv.iter().any(|a| a == "--rm"), "missing --rm");
        assert!(argv.iter().any(|a| a == "-i"), "missing -i");
    }

    #[test]
    fn tau_env_vars_forwarded() {
        let plan = plan_from(json!([]));
        let envs = vec![("TAU_FOO".to_string(), "bar".to_string())];
        let argv = build_run_args(
            &plan,
            ResolvedRuntime::Docker,
            "/bin/echo",
            &[],
            &envs,
            None,
        );
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
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
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
        let argv = build_run_args(
            &plan,
            ResolvedRuntime::Docker,
            "/bin/echo",
            &[],
            &envs,
            None,
        );
        let pos = argv
            .iter()
            .position(|a| a == "-e")
            .expect("-e flag present for RUST_LOG");
        assert_eq!(argv[pos + 1], "RUST_LOG=trace");
    }

    #[test]
    fn proxy_args_added_when_network_http_present() {
        let plan = plan_from(json!([{
            "kind": "net.http",
            "hosts": ["api.example.com"],
            "methods": ["GET"]
        }]));
        let proxy_cfg = ProxyConfig {
            sock_path: std::path::PathBuf::from("/tmp/tau-proxy-12345-0.sock"),
            bridge_path: "/usr/local/bin/tau-net-bridge".to_string(),
        };
        let argv = build_run_args(
            &plan,
            ResolvedRuntime::Docker,
            "/bin/echo",
            &[],
            &[],
            Some(&proxy_cfg),
        );
        // HTTPS_PROXY env must be present.
        assert!(
            argv.iter()
                .any(|a| a == "HTTPS_PROXY=http://127.0.0.1:8443"),
            "expected HTTPS_PROXY in argv: {argv:?}"
        );
        // Proxy socket bind-mount must be present.
        assert!(
            argv.iter().any(|a| a.contains("/run/tau-proxy.sock")),
            "expected proxy socket mount in argv: {argv:?}"
        );
        // Bridge binary bind-mount must be present.
        assert!(
            argv.iter()
                .any(|a| a.contains("/usr/local/bin/tau-net-bridge:ro")),
            "expected bridge binary mount in argv: {argv:?}"
        );
        // tau-net-bridge wrapper must appear after the image.
        let bridge_pos = argv
            .iter()
            .position(|a| a == "/usr/local/bin/tau-net-bridge")
            .expect("tau-net-bridge wrapper in argv");
        let image_pos = argv
            .iter()
            .position(|a| a == DEFAULT_BASE_IMAGE)
            .expect("image in argv");
        assert!(bridge_pos > image_pos, "bridge wrapper must follow image");
    }

    #[test]
    fn no_proxy_args_when_no_network_http() {
        let plan = plan_from(json!([]));
        let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
        assert!(
            !argv.iter().any(|a| a.contains("HTTPS_PROXY=")),
            "HTTPS_PROXY must not appear without network http: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a.contains("tau-proxy.sock")),
            "proxy socket must not appear without network http: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a.contains("tau-net-bridge")),
            "tau-net-bridge must not appear without network http: {argv:?}"
        );
    }
}
