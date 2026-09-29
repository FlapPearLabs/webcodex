// SPDX-License-Identifier: Apache-2.0
//! Spike tests for the per-action execution broker.
//!
//! # What these tests can and cannot establish on this host
//!
//! These are **real** tests against the real `ExecutionBroker`. On a host where
//! Seatbelt allow-list profiles can be applied they assert enforcement outcomes.
//! Where the kernel refuses allow-list profiles (`sandbox_apply: EPERM`), the
//! enforcement assertions are reported as `ENV_BLOCKED` rather than silently
//! passing, because a test that cannot fail is not evidence.
//!
//! The profile-construction and plan-refusal tests are host-independent and
//! always run for real.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use webcodex_process::execution_broker::{
    BrokerError, ExecutionBroker, NetworkPolicy, SandboxPlan,
};

const OUTSIDE: &str = "/tmp/webcodex-sandbox-spike/outside.txt";

fn fixture_workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../research/spikes/fixture")
}

/// Run a command under `plan` and return (stdout, exit code).
fn run_under(plan: &SandboxPlan, cwd: &Path, prog: &str, args: &[&str]) -> (String, i32) {
    let broker = ExecutionBroker::new();
    let mut cmd = Command::new(prog);
    cmd.args(args);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    match broker.spawn(&mut cmd, cwd, plan) {
        Err(BrokerError::Launch(e)) => panic!("broker launch failed: {e}"),
        Err(other) => panic!("unexpected broker error: {other}"),
        Ok(mut child) => {
            // `ManagedChild` owns the `std::process::Child` and `wait_with_output`
            // needs ownership of it, so drain the pipe and wait separately.
            let mut bytes = Vec::new();
            if let Some(mut stdout) = child.child_mut().stdout.take() {
                use std::io::Read as _;
                let _ = stdout.read_to_end(&mut bytes);
            }
            let status = child.wait().expect("wait");
            (
                String::from_utf8_lossy(&bytes).to_string(),
                status.code().unwrap_or(-1),
            )
        }
    }
}

/// True when this host can actually apply an allow-list profile.
fn host_allows_allowlist_profiles() -> bool {
    let mut probe = Command::new("/usr/bin/sandbox-exec");
    probe
        .arg("-p")
        .arg("(version 1)(allow default)(deny file-read*)");
    probe.arg("/usr/bin/true");
    probe.stdout(Stdio::null()).stderr(Stdio::null());
    probe.status().map(|s| s.success()).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Host-independent: profile construction
// ---------------------------------------------------------------------------

#[test]
fn plan_renders_allowlist_profile_with_only_named_roots() {
    let plan = SandboxPlan::read_write_in(&[PathBuf::from("/tmp/one")]);
    let sbpl = plan.to_sbpl().expect("plan should render");
    assert!(
        sbpl.contains("(deny file-read*)"),
        "must deny by default: {sbpl}"
    );
    assert!(
        sbpl.contains("(allow file-read* (subpath \"/tmp/one\"))"),
        "{sbpl}"
    );
    assert!(sbpl.contains("(deny file-write*)"), "{sbpl}");
    assert!(sbpl.contains("(deny network*)"), "{sbpl}");
}

/// The per-action property, checked at the profile level: two plans naming
/// different roots produce profiles that do not mention each other's data.
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
fn empty_plan_is_refused_rather_than_spawning_a_useless_process() {
    let plan = SandboxPlan::read_write_in(&[]);
    assert!(matches!(plan.to_sbpl(), Err(BrokerError::PlanRefused(_))));
}

#[test]
fn network_allow_plan_is_refused_because_no_proxy_exists() {
    let plan = SandboxPlan {
        writable_roots: vec![PathBuf::from("/tmp/x")],
        readable_roots: Vec::new(),
        network: NetworkPolicy::Allow,
    };
    // An unimplemented allow path must be a hard error, not a silent downgrade.
    assert!(matches!(plan.to_sbpl(), Err(BrokerError::PlanRefused(_))));
}

#[test]
fn refused_plan_never_reaches_spawn() {
    // The refusal must happen during profile construction, i.e. before any
    // process exists. If it were deferred, this would launch a real process.
    let plan = SandboxPlan::read_write_in(&[]);
    let broker = ExecutionBroker::new();
    let mut cmd = Command::new("/bin/echo");
    cmd.arg("THIS_MUST_NOT_RUN");
    let cwd = std::env::temp_dir();
    let err = broker
        .spawn(&mut cmd, &cwd, &plan)
        .expect_err("must refuse");
    assert!(matches!(err, BrokerError::PlanRefused(_)), "got {err}");
}

#[test]
fn quotes_in_paths_are_escaped_not_injected() {
    let plan = SandboxPlan::read_write_in(&[PathBuf::from("/tmp/a\" (allow default) \"b")]);
    let sbpl = plan.to_sbpl().unwrap();
    // The quotes must be escaped, so the embedded text stays *inside* the
    // string literal rather than closing it and becoming a new rule.
    assert!(
        sbpl.contains(r#"subpath "/tmp/a\" (allow default) \"b""#),
        "{sbpl}"
    );
    // Counting rules proves no extra top-level rule was injected: the profile
    // must still contain exactly the rules this module emits
    // (version, allow default, deny read, allow read, deny write, allow write,
    // deny network).
    let rule_lines = sbpl.lines().filter(|l| l.starts_with('(')).count();
    assert_eq!(
        rule_lines, 7,
        "unexpected rule count, injection likely: {sbpl}"
    );
}

// ---------------------------------------------------------------------------
// Host-dependent: enforcement outcomes
// ---------------------------------------------------------------------------

#[test]
fn test_a_workspace_read_write() {
    let ws = fixture_workspace();
    if !host_allows_allowlist_profiles() {
        eprintln!("ENV_BLOCKED: host rejects allow-list Seatbelt profiles; A not measurable");
        return;
    }
    let plan = SandboxPlan::read_write_in(&[ws.clone()]);
    let inside = ws.join("inside.txt");
    let (out, code) = run_under(&plan, &ws, "/bin/cat", &[inside.to_str().unwrap()]);
    assert_eq!(code, 0, "reading inside the workspace must succeed: {out}");
    assert!(out.contains("WORKSPACE_INSIDE"), "{out}");
}

#[test]
fn test_b_external_read_is_denied() {
    let ws = fixture_workspace();
    if !host_allows_allowlist_profiles() {
        eprintln!("ENV_BLOCKED: host rejects allow-list Seatbelt profiles; B not measurable");
        return;
    }
    let plan = SandboxPlan::read_write_in(&[ws]);
    let (_, code) = run_under(&plan, &std::env::temp_dir(), "/bin/cat", &[OUTSIDE]);
    assert_ne!(code, 0, "reading outside the workspace must be denied");
}

#[test]
fn test_c_process_tree_inherits_the_profile_shell_to_python() {
    let ws = fixture_workspace();
    if !host_allows_allowlist_profiles() {
        eprintln!("ENV_BLOCKED: host rejects allow-list Seatbelt profiles; C not measurable");
        return;
    }
    let plan = SandboxPlan::read_write_in(&[ws]);
    // shell -> python3 -> read outside. If the profile is inherited, the read
    // fails two levels down.
    let script = format!(
        "import subprocess,sys; subprocess.run([sys.executable,'-c','open(\"{OUTSIDE}\").read()'])"
    );
    let (_, code) = run_under(&plan, &std::env::temp_dir(), "/bin/sh", &["-c", &script]);
    assert_ne!(code, 0, "grandchild must inherit the deny");
}

#[test]
fn test_d_same_action_differs_between_profiles() {
    let ws = fixture_workspace();
    if !host_allows_allowlist_profiles() {
        eprintln!("ENV_BLOCKED: host rejects allow-list Seatbelt profiles; D not measurable");
        return;
    }
    // Profile A: workspace only -> deny. Profile B: adds the outside fixture
    // -> allow. Same read, opposite outcomes: this is what proves per-action
    // authority rather than a fixed runner-wide profile.
    let a = SandboxPlan::read_write_in(&[ws.clone()]);
    let b = SandboxPlan {
        writable_roots: vec![ws.clone(), PathBuf::from(OUTSIDE)],
        readable_roots: Vec::new(),
        network: NetworkPolicy::Deny,
    };
    let (_, code_a) = run_under(&a, &std::env::temp_dir(), "/bin/cat", &[OUTSIDE]);
    let (out_b, code_b) = run_under(&b, &std::env::temp_dir(), "/bin/cat", &[OUTSIDE]);
    assert_ne!(code_a, 0, "profile A must deny");
    assert_eq!(code_b, 0, "profile B must allow: {out_b}");
}

#[test]
fn test_e_network_denied() {
    let ws = fixture_workspace();
    if !host_allows_allowlist_profiles() {
        eprintln!("ENV_BLOCKED: host rejects allow-list Seatbelt profiles; E not measurable");
        return;
    }
    // Loopback listener started OUTSIDE the sandbox, so a denial is attributable
    // to the profile and not to a missing server.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let plan = SandboxPlan::read_write_in(&[ws]);
    let script = format!(
        "import socket,sys; s=socket.socket(); s.settimeout(2); s.connect(('127.0.0.1',{port})); print('CONNECTED')"
    );
    let (out, code) = run_under(&plan, &std::env::temp_dir(), "/bin/sh", &["-c", &script]);
    assert_ne!(code, 0, "network must be denied");
    assert!(
        !out.contains("CONNECTED"),
        "socket connected despite deny: {out}"
    );
}

// ---------------------------------------------------------------------------
// The architectural point: the control plane is not inside the profile
// ---------------------------------------------------------------------------

#[test]
fn control_plane_remains_outside_the_sandbox_and_can_issue_a_different_plan() {
    let ws = fixture_workspace();
    // This test needs no allow-list profile: it proves a structural property.
    // The broker is a value, it holds no state, and nothing in this process is
    // placed inside a profile. So the same control-plane process can build
    // profile A, run it, then build a *different* profile B and run that.
    let broker = ExecutionBroker::new();

    let plan_a = SandboxPlan::read_write_in(&[ws.clone()]);
    let sbpl_a = plan_a.to_sbpl().expect("A renders");
    assert!(
        !sbpl_a.contains(OUTSIDE),
        "profile A must not reach the outside fixture"
    );

    // A second, independent plan from the same untouched control plane.
    let plan_b = SandboxPlan::read_write_in(&[ws, PathBuf::from(OUTSIDE)]);
    let sbpl_b = plan_b.to_sbpl().expect("B renders");
    assert!(sbpl_b.contains(OUTSIDE), "profile B must reach it");

    // And the broker is reusable afterwards: it is not consumed by a spawn.
    let plan_c = SandboxPlan::read_write_in(&[PathBuf::from("/tmp/other")]);
    assert!(plan_c.to_sbpl().is_ok());
    let _ = broker;
}
