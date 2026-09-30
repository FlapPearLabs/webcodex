// SPDX-License-Identifier: Apache-2.0
//! Spike round 2 tests: does the broker preserve `Command` execution semantics?
//!
//! # Why these tests exist
//!
//! Round 1's broker took a `&mut Command` and copied three fields out of it.
//! Everything else the caller had configured — `stdin`/`stdout`/`stderr` and
//! the entire environment — was silently discarded. A caller who wrote
//! `cmd.stdout(Stdio::piped())` got a child whose stdout was not piped, and
//! nothing reported an error.
//!
//! # Why they cannot be ENV_BLOCKED
//!
//! Round 1's enforcement tests (A-E) were unverifiable on the spike host because
//! the kernel refuses restrictive Seatbelt profiles. These fidelity tests do not
//! need one. They run under a **permissive** plan, because what they check is
//! the broker's command construction and process handling, not profile
//! enforcement. If a future host accepts restrictive profiles, the A-E tests
//! start asserting; these already assert either way.
//!
//! `(allow default)` is a real profile that really is applied by a real
//! `sandbox-exec` process — the child really is launched through the broker's
//! launcher, with the launcher as its parent. Only the *restrictions* are
//! absent.

use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use webcodex_process::execution_broker::{
    BrokerError, EnvPolicy, ExecutionBroker, NetworkPolicy, SandboxPlan, SpawnSpec, StreamPolicy,
};

const OUTSIDE: &str = "/tmp/webcodex-sandbox-spike/outside.txt";

fn fixture_workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../research/spikes/fixture")
}

fn tmpdir() -> PathBuf {
    std::env::temp_dir()
}

/// The plan used by command-fidelity tests.
///
/// These tests assert that the broker hands the caller the execution semantics
/// it asked for — pipes, cwd, environment, exit status, lifecycle. They are not
/// about confinement, and they must not be skipped on a host that refuses
/// restrictive profiles, so they run under a bare `(allow default)` profile
/// through the real launcher.
fn fidelity_plan() -> SandboxPlan {
    SandboxPlan::UnconfinedForFidelityTesting
}

fn read_stdout(child: &mut webcodex_process::ManagedChild) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut stdout) = child.child_mut().stdout.take() {
        let _ = stdout.read_to_end(&mut bytes);
    }
    bytes
}

fn read_stderr(child: &mut webcodex_process::ManagedChild) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut stderr) = child.child_mut().stderr.take() {
        let _ = stderr.read_to_end(&mut bytes);
    }
    bytes
}

/// Run `spec` through the broker, returning (stdout, stderr, exit code).
fn run(spec: &SpawnSpec) -> (Vec<u8>, Vec<u8>, i32) {
    let broker = ExecutionBroker::new();
    match broker.spawn_unconfined_for_fidelity_testing(spec) {
        Err(BrokerError::Launch(e)) => panic!("broker launch failed: {e}"),
        Err(other) => panic!("unexpected broker error: {other}"),
        Ok(mut child) => {
            let out = read_stdout(&mut child);
            let err = read_stderr(&mut child);
            let status = child.wait().expect("wait");
            (out, err, status.code().unwrap_or(-1))
        }
    }
}

fn sh(script: &str) -> SpawnSpec {
    SpawnSpec::new("/bin/sh", tmpdir(), fidelity_plan())
        .arg("-c")
        .arg(script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped)
}

// ---------------------------------------------------------------------------
// F1-F8: command fidelity. These run for real on any host.
// ---------------------------------------------------------------------------

/// F1 — a requested stdout pipe reaches the child and comes back to the caller.
///
/// This is the exact regression round 1 had: the pipe was requested on the
/// caller's `Command` and dropped by the broker.
#[test]
fn f1_stdout_pipe_preserved() {
    let spec = sh("printf 'F1_MARKER'");
    assert_eq!(
        spec.stdout,
        StreamPolicy::Piped,
        "spec must request the pipe"
    );
    let (out, _, code) = run(&spec);
    assert_eq!(code, 0, "child must exit cleanly");
    assert_eq!(
        String::from_utf8_lossy(&out).trim(),
        "F1_MARKER",
        "stdout must survive the broker"
    );
}

/// F2 — stderr is a separate stream and is not merged into stdout.
#[test]
fn f2_stderr_pipe_preserved() {
    let spec = sh("printf 'TO_STDERR' 1>&2");
    let (out, err, code) = run(&spec);
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8_lossy(&err).trim(), "TO_STDERR");
    assert!(
        out.is_empty(),
        "stderr must not leak into stdout: {}",
        String::from_utf8_lossy(&out)
    );
}

/// F3 — stdin is piped and the parent can write to it.
#[test]
fn f3_stdin_pipe_preserved() {
    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new("/bin/cat", tmpdir(), fidelity_plan())
        .arg("-")
        .stdin(StreamPolicy::Piped)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    let mut child = broker
        .spawn_unconfined_for_fidelity_testing(&spec)
        .expect("spawn");
    {
        use std::io::Write as _;
        let mut stdin = child.child_mut().stdin.take().expect("stdin pipe");
        stdin.write_all(b"F3_THROUGH_STDIN").expect("write stdin");
    } // dropped -> EOF so `cat` terminates
    let out = read_stdout(&mut child);
    let code = child.wait().expect("wait").code().unwrap_or(-1);
    assert_eq!(code, 0, "cat must exit cleanly");
    assert_eq!(
        String::from_utf8_lossy(&out),
        "F3_THROUGH_STDIN",
        "bytes written to the child's stdin must come back on stdout"
    );
}

/// F4 — the working directory the caller asked for is the directory the child
/// actually runs in. The `sandbox-exec` launcher sits between, so this is not
/// automatic.
#[test]
fn f4_cwd_preserved() {
    let probe = tmpdir();
    let spec = SpawnSpec::new("/bin/sh", probe.clone(), fidelity_plan())
        .arg("-c")
        .arg("pwd")
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    let (out, _, code) = run(&spec);
    assert_eq!(code, 0);
    let reported = String::from_utf8_lossy(&out).trim().to_string();
    // macOS reports /private/var for /var; accept either form.
    let expected = probe
        .canonicalize()
        .unwrap_or_else(|_| probe.clone())
        .to_string_lossy()
        .to_string();
    assert!(
        reported == probe.to_string_lossy() || reported == expected,
        "cwd mismatch: child reported {reported}, spec asked for {}",
        probe.to_string_lossy()
    );
}

/// F5 — an explicitly requested variable reaches the child.
#[test]
fn f5_explicit_env_propagated() {
    let spec = sh("printf '%s' \"$F5_VAR\"").env_var("F5_VAR", "F5_VALUE");
    let (out, _, code) = run(&spec);
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8_lossy(&out), "F5_VALUE");
}

/// F6 — a secret in the *broker's* environment does not reach the child under
/// the default env policy.
///
/// This is the property that makes the default safe: a sandboxed action must
/// not inherit the runner's credentials merely because the caller did not
/// mention them.
#[test]
fn f6_runner_secret_env_not_inherited_by_default() {
    // The variable name is unique to this test; the value is a literal in this
    // file, not a real credential.
    std::env::set_var("F6_RUNNER_SECRET", "F6_LEAKED");
    let spec = sh("printf '%s' \"${F6_RUNNER_SECRET:-ABSENT}\"");
    assert_eq!(spec.env, EnvPolicy::Minimal, "default must be Minimal");
    let (out, _, code) = run(&spec);
    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8_lossy(&out),
        "ABSENT",
        "the runner's environment must not leak into a brokered child"
    );
    std::env::remove_var("F6_RUNNER_SECRET");
}

/// F6b — `EnvPolicy::Inherit` is the explicit opt-in, and it does carry the
/// variable across. Without this, F6 could pass for the wrong reason.
#[test]
fn f6b_inherit_is_explicit_opt_in() {
    std::env::set_var("F6B_MARKER", "F6B_PRESENT");
    let spec = sh("printf '%s' \"${F6B_MARKER:-ABSENT}\"").env(EnvPolicy::Inherit);
    let (out, _, code) = run(&spec);
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8_lossy(&out), "F6B_PRESENT");
    std::env::remove_var("F6B_MARKER");
}

/// F7 — the child's exit status is reported unchanged.
#[test]
fn f7_exit_status_preserved() {
    let spec = sh("exit 42");
    let (_, _, code) = run(&spec);
    assert_eq!(
        code, 42,
        "exit status must pass through the launcher unchanged"
    );
}

/// F8 — `ManagedChild` lifecycle still works: the child is observable while
/// running, and `terminate_tree` reaches it through the `sandbox-exec` layer.
#[test]
fn f8_process_lifecycle_unchanged() {
    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new("/bin/sleep", tmpdir(), fidelity_plan())
        .arg("30")
        .stdout(StreamPolicy::Null)
        .stderr(StreamPolicy::Null);

    let mut child = broker
        .spawn_unconfined_for_fidelity_testing(&spec)
        .expect("spawn");
    assert!(child.id() > 0, "managed child must expose a pid");
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "child must still be running"
    );
    child.terminate_tree().expect("terminate_tree");
    let exited = child
        .wait_tree_exit(Duration::from_secs(10))
        .expect("wait_tree_exit");
    assert!(
        exited,
        "process tree must actually exit after terminate_tree"
    );
}

// ---------------------------------------------------------------------------
// Broker-owned command construction
// ---------------------------------------------------------------------------

/// The broker builds the command; the caller cannot smuggle configuration in
/// behind its back. This is the structural difference from round 1.
#[test]
fn broker_owns_command_construction() {
    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new("/bin/echo", tmpdir(), fidelity_plan()).arg("hello");
    let command = broker.build_command(&spec).expect("build");
    let rendered: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    // sandbox-exec -p <profile> /bin/echo hello
    assert_eq!(command.get_program(), "/usr/bin/sandbox-exec");
    assert_eq!(rendered.first().map(String::as_str), Some("-p"));
    assert_eq!(
        rendered.get(2).map(String::as_str),
        Some("/bin/echo"),
        "program must be forwarded after the profile"
    );
    assert_eq!(rendered.last().map(String::as_str), Some("hello"));
}

/// A refused plan must fail before any process exists.
#[test]
fn refused_plan_never_reaches_spawn() {
    let broker = ExecutionBroker::new();
    let plan = SandboxPlan::read_write_in(&[]);
    let spec = SpawnSpec::new("/bin/echo", tmpdir(), plan).arg("THIS_MUST_NOT_RUN");
    let err = broker.spawn(&spec).expect_err("must refuse");
    assert!(matches!(err, BrokerError::PlanRefused(_)), "got {err}");
}

/// A network-allow plan is a hard error, never a silent downgrade to deny.
#[test]
fn network_allow_plan_is_refused_because_no_proxy_exists() {
    let plan = SandboxPlan::Confined {
        writable_roots: vec![PathBuf::from("/tmp")],
        readable_roots: Vec::new(),
        network: NetworkPolicy::Allow,
    };
    assert!(matches!(plan.to_sbpl(), Err(BrokerError::PlanRefused(_))));
}

/// Path quoting is escaped, and the rule count proves no rule was injected.
#[test]
fn quotes_in_paths_are_escaped_not_injected() {
    let plan = SandboxPlan::read_write_in(&[PathBuf::from("/tmp/a\" (allow default) \"b")]);
    let sbpl = plan.to_sbpl().expect("render");
    assert!(
        sbpl.contains(r#"subpath "/tmp/a\" (allow default) \"b""#),
        "{sbpl}"
    );
    let rule_lines = sbpl.lines().filter(|l| l.starts_with('(')).count();
    assert_eq!(
        rule_lines, 7,
        "unexpected rule count, injection likely: {sbpl}"
    );
}

// ---------------------------------------------------------------------------
// Per-action independence and control-plane separation (structural)
// ---------------------------------------------------------------------------

#[test]
fn two_plans_are_independent_at_profile_level() {
    let a = SandboxPlan::read_write_in(&[PathBuf::from("/tmp/workspace")]);
    let b = SandboxPlan::read_write_in(&[PathBuf::from(OUTSIDE)]);
    let sbpl_a = a.to_sbpl().unwrap();
    let sbpl_b = b.to_sbpl().unwrap();
    assert!(
        !sbpl_a.contains(OUTSIDE),
        "profile A must not grant the outside fixture"
    );
    assert!(
        sbpl_b.contains(OUTSIDE),
        "profile B must grant the outside fixture"
    );
    assert_ne!(sbpl_a, sbpl_b);
}

#[test]
fn control_plane_can_issue_different_plans_and_is_never_consumed() {
    let broker = ExecutionBroker::new();
    let ws = PathBuf::from("/tmp/workspace");

    let a = broker
        .build_command(&SpawnSpec::new(
            "/bin/true",
            ws.clone(),
            SandboxPlan::read_write_in(&[ws.clone()]),
        ))
        .expect("A builds");
    let sbpl_a = a.get_args().nth(1).unwrap().to_string_lossy().to_string();
    assert!(
        !sbpl_a.contains(OUTSIDE),
        "plan A must not reach the outside fixture"
    );

    let b = broker
        .build_command(&SpawnSpec::new(
            "/bin/true",
            ws.clone(),
            SandboxPlan::read_write_in(&[ws.clone(), PathBuf::from(OUTSIDE)]),
        ))
        .expect("B builds");
    let sbpl_b = b.get_args().nth(1).unwrap().to_string_lossy().to_string();
    assert!(sbpl_b.contains(OUTSIDE), "plan B must reach it");

    // Still usable: the broker is a value and a spawn does not consume it.
    let c = broker
        .build_command(&SpawnSpec::new(
            "/bin/true",
            ws,
            SandboxPlan::read_write_in(&[PathBuf::from("/tmp/other")]),
        ))
        .expect("C builds");
    assert_eq!(c.get_program(), "/usr/bin/sandbox-exec");
}

// ---------------------------------------------------------------------------
// Round 1 enforcement experiments A-E.
//
// Status on the spike host: the kernel refuses every restrictive Seatbelt
// profile with `sandbox_apply: Operation not permitted`, so these cannot
// measure anything here. They are retained and report ENV_BLOCKED via a
// runtime probe, so they start asserting on a host that allows narrowing.
// ---------------------------------------------------------------------------

fn host_allows_restrictive_profiles() -> bool {
    for profile in [
        "(version 1)(allow default)(deny network*)",
        "(version 1)(allow default)(deny file-read*)",
    ] {
        let ok = std::process::Command::new("/usr/bin/sandbox-exec")
            .arg("-p")
            .arg(profile)
            .arg("/usr/bin/true")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return false;
        }
    }
    true
}

#[test]
fn test_a_workspace_read_write() {
    if !host_allows_restrictive_profiles() {
        eprintln!("ENV_BLOCKED: host refuses restrictive Seatbelt profiles; A not measurable");
        return;
    }
    let ws = fixture_workspace();
    let spec = SpawnSpec::new(
        "/bin/cat",
        ws.clone(),
        SandboxPlan::read_write_in(&[ws.clone()]),
    )
    .arg(ws.join("inside.txt").to_str().unwrap())
    .stdout(StreamPolicy::Piped);
    let (out, _, code) = run(&spec);
    assert_eq!(code, 0, "reading inside the workspace must succeed");
    assert!(String::from_utf8_lossy(&out).contains("WORKSPACE_INSIDE"));
}

#[test]
fn test_b_external_read_is_denied() {
    if !host_allows_restrictive_profiles() {
        eprintln!("ENV_BLOCKED: host refuses restrictive Seatbelt profiles; B not measurable");
        return;
    }
    let ws = fixture_workspace();
    let spec = SpawnSpec::new("/bin/cat", tmpdir(), SandboxPlan::read_write_in(&[ws]))
        .arg(OUTSIDE)
        .stdout(StreamPolicy::Piped);
    let (_, _, code) = run(&spec);
    assert_ne!(code, 0, "reading outside the workspace must be denied");
}

#[test]
fn test_c_process_tree_inherits_the_profile_shell_to_python() {
    if !host_allows_restrictive_profiles() {
        eprintln!("ENV_BLOCKED: host refuses restrictive Seatbelt profiles; C not measurable");
        return;
    }
    let spec = sh(&format!("python3 -c 'open(\"{OUTSIDE}\").read()'"));
    let (_, _, code) = run(&spec);
    assert_ne!(code, 0, "grandchild must inherit the deny");
}

#[test]
fn test_d_same_action_differs_between_profiles() {
    if !host_allows_restrictive_profiles() {
        eprintln!("ENV_BLOCKED: host refuses restrictive Seatbelt profiles; D not measurable");
        return;
    }
    let ws = fixture_workspace();
    let a = SpawnSpec::new(
        "/bin/cat",
        tmpdir(),
        SandboxPlan::read_write_in(&[ws.clone()]),
    )
    .arg(OUTSIDE)
    .stdout(StreamPolicy::Piped);
    let b = SpawnSpec::new(
        "/bin/cat",
        tmpdir(),
        SandboxPlan::Confined {
            writable_roots: vec![ws, PathBuf::from(OUTSIDE)],
            readable_roots: Vec::new(),
            network: NetworkPolicy::Deny,
        },
    )
    .arg(OUTSIDE)
    .stdout(StreamPolicy::Piped);
    let (_, _, code_a) = run(&a);
    let (_, _, code_b) = run(&b);
    assert_ne!(code_a, 0, "profile A must deny");
    assert_eq!(code_b, 0, "profile B must allow");
}

#[test]
fn test_e_network_denied() {
    if !host_allows_restrictive_profiles() {
        eprintln!("ENV_BLOCKED: host refuses restrictive Seatbelt profiles; E not measurable");
        return;
    }
    // Listener bound OUTSIDE the sandbox, so a denial is attributable to the
    // profile and not to a missing server.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let script = format!(
        "python3 -c 'import socket;s=socket.socket();s.settimeout(2); \
         s.connect((\"127.0.0.1\",{port}));print(\"CONNECTED\")'"
    );
    let spec = sh(&script);
    let (out, _, code) = run(&spec);
    assert_ne!(code, 0, "network must be denied");
    assert!(!String::from_utf8_lossy(&out).contains("CONNECTED"));
}
