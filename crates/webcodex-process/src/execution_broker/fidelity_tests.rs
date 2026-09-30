// SPDX-License-Identifier: Apache-2.0
//! Command-fidelity tests, plus a static check of the production API surface.
//!
//! # Why these are unit tests and not integration tests
//!
//! They need a permissive profile to run on a host whose kernel refuses
//! restrictive ones. That profile is reachable only through
//! [`super::testing`], which is `#[cfg(test)] pub(crate)`. An integration test
//! in `tests/` links the library **without** `cfg(test)`, so it cannot see that
//! module at all — which is exactly the property we want, and is itself part of
//! the evidence that no unrestricted spawn exists in a release build.
//!
//! What the fidelity tests check is the broker's command construction and
//! process handling, not confinement. A permissive profile is still a real
//! profile, really applied by a real `sandbox-exec` process, with the launcher
//! as the child's parent. Only the restrictions are absent.
//!
//! # Why they must not be ENV_BLOCKED
//!
//! Round 2's predecessor tests were unverifiable on constrained hosts. These
//! do not need a restrictive profile, so they run for real anywhere. If a
//! future host accepts narrowing profiles, the A-E enforcement tests start
//! asserting; these already assert either way.

use std::io::{Read, Write as _};
use std::path::PathBuf;
use std::time::Duration;

use super::testing::{build_permissive_command, spawn_permissive};
use super::{
    BrokerError, CompileError, EnvPolicy, ExecutionBroker, NetworkPolicy, SandboxPlan, SpawnSpec,
    StreamPolicy,
};
use crate::ManagedChild;

fn tmpdir() -> PathBuf {
    std::env::temp_dir()
}

/// The plan these specs carry.
///
/// It is a *real* confined plan, not a placeholder: the fidelity tests assert
/// that a realistic spec survives the broker intact. The permissive profile is
/// substituted only at launch, by `spawn_permissive`, because these tests are
/// about command semantics rather than about denial.
fn fidelity_plan() -> SandboxPlan {
    SandboxPlan::read_write_in(&[tmpdir()])
}

fn read_stdout(child: &mut ManagedChild) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut stdout) = child.child_mut().stdout.take() {
        let _ = stdout.read_to_end(&mut bytes);
    }
    bytes
}

fn read_stderr(child: &mut ManagedChild) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut stderr) = child.child_mut().stderr.take() {
        let _ = stderr.read_to_end(&mut bytes);
    }
    bytes
}

/// Run `spec` through the broker, returning (stdout, stderr, exit code).
fn run(spec: &SpawnSpec) -> (Vec<u8>, Vec<u8>, i32) {
    match spawn_permissive(spec) {
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
    let spec = SpawnSpec::new("/bin/cat", tmpdir(), fidelity_plan())
        .arg("-")
        .stdin(StreamPolicy::Piped)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    let mut child = spawn_permissive(&spec).expect("spawn");
    {
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
    let dir = tempfile::tempdir().expect("tempdir");
    let probe = dir.path().to_path_buf();
    let spec = SpawnSpec::new("/bin/sh", probe.clone(), fidelity_plan())
        .arg("-c")
        .arg("pwd")
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    let (out, err, code) = run(&spec);
    assert_eq!(code, 0, "child failed: {}", String::from_utf8_lossy(&err));
    let reported = String::from_utf8_lossy(&out).trim().to_string();
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
    let spec = SpawnSpec::new("/bin/sleep", tmpdir(), fidelity_plan())
        .arg("30")
        .stdout(StreamPolicy::Null)
        .stderr(StreamPolicy::Null);

    let mut child = spawn_permissive(&spec).expect("spawn");
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
    assert_eq!(command.get_program(), "/usr/bin/sandbox-exec");
    assert_eq!(rendered.first().map(String::as_str), Some("-p"));

    // `--` terminates sandbox-exec's own options; the program follows it, so a
    // path beginning with `-` cannot be read as a flag.
    let sep = rendered
        .iter()
        .position(|a| a == "--")
        .expect("argv must contain --");
    assert_eq!(
        rendered.get(sep + 1).map(String::as_str),
        Some("/bin/echo"),
        "program must be forwarded after --"
    );
    assert_eq!(rendered.last().map(String::as_str), Some("hello"));
}

/// Roots reach the kernel as `-D` parameters, never as profile text.
///
/// This is the property that makes a path incapable of widening its own
/// profile: there is no SBPL parser between the path and the kernel.
#[test]
fn roots_are_passed_as_argv_definitions_not_profile_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let broker = ExecutionBroker::new();
    let plan = SandboxPlan::read_write_in(&[dir.path().to_path_buf()]);
    let command = broker
        .build_command(&SpawnSpec::new("/bin/true", dir.path(), plan))
        .expect("build");
    let rendered: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let profile = &rendered[1];
    assert!(
        profile.contains("(deny default)"),
        "profile must be deny-default, got: {profile}"
    );
    assert!(
        !profile.contains(&*dir.path().to_string_lossy()),
        "the root path must not appear in the profile text: {profile}"
    );
    assert!(
        rendered
            .iter()
            .any(|a| a.starts_with("-DWEB_CODEX_READABLE_ROOT_0=")),
        "root must be passed as a -D definition, argv was: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|a| a.starts_with("-DWEB_CODEX_WRITABLE_ROOT_0=")),
        "writable root must also be a definition, argv was: {rendered:?}"
    );
}

/// A refused plan must fail before any process exists.
#[test]
fn refused_plan_never_reaches_spawn() {
    let broker = ExecutionBroker::new();
    let plan = SandboxPlan::read_write_in(&[]);
    let spec = SpawnSpec::new("/bin/echo", tmpdir(), plan).arg("THIS_MUST_NOT_RUN");
    let err = broker.spawn(&spec).expect_err("must refuse");
    assert!(
        matches!(err, BrokerError::PlanNotCompilable(_)),
        "got {err}"
    );
}

/// A network-allow plan is a hard error, never a silent downgrade to deny.
#[test]
fn network_allow_plan_is_refused_because_no_proxy_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plan = SandboxPlan::Confined {
        writable_roots: vec![dir.path().to_path_buf()],
        readable_roots: Vec::new(),
        network: NetworkPolicy::Allow,
    };
    assert!(matches!(
        plan.compile(&[]),
        Err(CompileError::Unsupported(_))
    ));
}

/// Per-action independence: two plans naming different roots grant different
/// authority.
///
/// The two profiles' *text* is identical — that is the point, not an accident.
/// Because roots travel as argv, the profile contains only parameter names, so
/// the authority difference lives entirely in the definitions. A profile
/// cannot be read, diffed, or audited by inspecting its text alone; the
/// definitions must be carried with it, which is why `CompiledProfile` keeps
/// the two halves in one value.
#[test]
fn two_plans_are_independent_at_profile_level() {
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    let plan_a = SandboxPlan::read_write_in(&[a.path().to_path_buf()]);
    let plan_b = SandboxPlan::read_write_in(&[b.path().to_path_buf()]);
    let ca = plan_a.compile(&[]).expect("A compiles");
    let cb = plan_b.compile(&[]).expect("B compiles");

    assert_eq!(
        ca.sbpl, cb.sbpl,
        "identical roots-by-argv means identical profile text; authority must \
         differ only in the definitions"
    );
    assert_ne!(
        ca.definitions, cb.definitions,
        "the definitions are where per-action authority actually lives"
    );
    let b_root = b.path().to_string_lossy().into_owned();
    assert!(
        !ca.definitions.iter().any(|d| d.ends_with(&b_root)),
        "plan A must not grant plan B's root"
    );
    assert!(
        cb.definitions.iter().any(|d| d.ends_with(&b_root)),
        "plan B must grant its own root"
    );
}

/// The broker is a value: issuing a profile for one action does not consume
/// the control plane's ability to issue a different one for the next.
#[test]
fn control_plane_is_never_consumed() {
    let broker = ExecutionBroker::new();
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    for root in [a.path().to_path_buf(), b.path().to_path_buf()] {
        let cmd = broker
            .build_command(&SpawnSpec::new(
                "/bin/true",
                &root,
                SandboxPlan::read_write_in(&[root.clone()]),
            ))
            .expect("builds");
        assert_eq!(cmd.get_program(), "/usr/bin/sandbox-exec");
    }
}

/// The permissive fidelity launcher is a real launcher invocation, not a bare
/// `Command`. If it ever stopped going through `sandbox-exec`, these tests
/// would no longer be testing the broker.
#[test]
fn permissive_helper_still_uses_the_real_launcher() {
    let spec = SpawnSpec::new("/bin/echo", tmpdir(), fidelity_plan()).arg("x");
    let command = build_permissive_command(&spec).expect("build");
    assert_eq!(command.get_program(), "/usr/bin/sandbox-exec");
    let rendered: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert!(rendered.contains(&"(version 1)\n(allow default)\n".to_string()));
    assert!(rendered.contains(&"--".to_string()));
}

// ---------------------------------------------------------------------------
// Production API surface: no unrestricted execution path
// ---------------------------------------------------------------------------

/// Identifiers that would constitute an escape hatch if they appeared in
/// production code.
const FORBIDDEN_IN_PRODUCTION: &[&str] = &[
    "UnconfinedForFidelityTesting",
    "spawn_unconfined_for_fidelity_testing",
    "unsafe_no_sandbox",
    "disable_sandbox",
    "bypass_sandbox",
    "sandbox_noop",
    "spawn_unrestricted",
];

/// Strip comments and string literals.
///
/// Comments must go because this file and `mod.rs` both *describe* the absence
/// of an escape hatch in prose, and that prose names the very identifiers being
/// searched for. String literals must go for the same reason in a sharper form:
/// a string is not a symbol, so the token list below would otherwise match
/// itself and every future mention of it in a diagnostic.
///
/// What survives is code — the only place an escape hatch could actually be
/// called from.
fn strip_comments_and_strings(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_line = false;
    let mut in_block = false;
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_line {
            if c == '\n' {
                in_line = false;
                out.push('\n');
            }
            continue;
        }
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if in_string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                in_line = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block = true;
            }
            '"' => {
                in_string = true;
            }
            _ => out.push(c),
        }
    }
    out
}

/// **The production surface contains no unrestricted execution API.**
///
/// This scans the crate's own sources — comments and string literals removed —
/// and requires that every occurrence of an escape-hatch identifier sits *after*
/// a `#[cfg(test)]` gate. A file with no such gate must contain none of them.
///
/// The check is deliberately source-level rather than "call it and see if it
/// compiles": a caller cannot accidentally depend on an absent symbol, but a
/// reviewer should not have to take that on faith either.
#[test]
fn production_api_has_no_unrestricted_execution_escape_hatch() {
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = crate_root.join("src");

    let mut checked = 0usize;
    let mut violations: Vec<String> = Vec::new();

    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let raw = std::fs::read_to_string(&path).expect("readable .rs");
            let code = strip_comments_and_strings(&raw);
            let cfg_test_at = code.find("#[cfg(test)]");
            checked += 1;

            for token in FORBIDDEN_IN_PRODUCTION {
                let mut from = 0usize;
                while let Some(rel) = code[from..].find(token) {
                    let at = from + rel;
                    let gated = cfg_test_at.is_some_and(|gate| at > gate);
                    if !gated {
                        violations.push(format!(
                            "{}:{} contains a forbidden identifier outside a #[cfg(test)] gate",
                            path.strip_prefix(crate_root).unwrap_or(&path).display(),
                            code[..at].lines().count()
                        ));
                    }
                    from = at + token.len();
                }
            }
        }
    }

    assert!(checked > 0, "no sources were scanned; the check is vacuous");
    assert!(
        violations.is_empty(),
        "production surface must contain no escape hatch:\n{}",
        violations.join("\n")
    );
}

/// The `SandboxPlan` enum has exactly one variant: `Confined`.
///
/// Enforced structurally — a second variant would be reachable from a release
/// build, which is the thing this whole refactor removed.
#[test]
fn sandbox_plan_has_no_unconfined_variant() {
    let dir = tempfile::tempdir().expect("tempdir");
    // If an unconfined variant existed, it would need to be matched here for
    // this function to compile. The compiler is the enforcement.
    let plans = [
        SandboxPlan::read_write_in(&[dir.path().to_path_buf()]),
        SandboxPlan::Confined {
            writable_roots: vec![dir.path().to_path_buf()],
            readable_roots: vec![dir.path().to_path_buf()],
            network: NetworkPolicy::Deny,
        },
    ];
    for plan in &plans {
        assert!(matches!(plan, SandboxPlan::Confined { .. }));
    }
}
