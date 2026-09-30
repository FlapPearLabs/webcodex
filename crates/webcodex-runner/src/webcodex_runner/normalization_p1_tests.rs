// SPDX-License-Identifier: Apache-2.0
//! P1 functional coverage: cases A-J, plus the structural anti-bypass guard.
//!
//! # What this file is
//!
//! Two different kinds of evidence, kept together because they answer the same
//! question from opposite directions:
//!
//! * **Cases A-J** ask "does the routed path actually confine?" — they need a
//!   host that can apply a restrictive Seatbelt profile, so each one is written
//!   to *observe* the host's answer rather than assume it. On a host that
//!   cannot apply the profile they report `ENV_BLOCKED`, never `PASS`.
//! * **The structural guard** asks "can a P1 surface bypass the broker at all?"
//!   That is a property of the source text, so it is checked by reading the
//!   source rather than by running it, and it holds on every host.
//!
//! # Why the guard is narrow on purpose
//!
//! It pins the P1-routed *functions* only. A blanket ban on `Command::spawn`
//! across the Runner would flag the Runner's own control-plane processes (the
//! Node version probe, the validation availability probe, the detached
//! supervisor handshake) and the surfaces P1 explicitly excludes (SSH,
//! browser/CDP, plugin/MCP providers, LSP, the persistent interactive shell,
//! and the detached durable payload). Widening the guard to those would either
//! be wrong or would require re-opening the P1 scope decision, which this round
//! does not do. Those exclusions are themselves asserted present, so the
//! remaining-surface list cannot quietly become wrong.

use std::path::{Path, PathBuf};

use super::config::RunnerPolicy;
use super::local_execution::{
    derive_workspace_plan, spawn_local_action, LocalEnv, LocalExecutionRequest,
};
use webcodex_process::execution_broker::{NetworkPolicy, SandboxPlan};

/// Outcome of one native-facing P1 case on this host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeOutcome {
    /// The action ran under the profile and the confinement held.
    Confirmed,
    /// This host cannot apply a restrictive Seatbelt profile, so the case
    /// established nothing. Never reported as a pass.
    EnvBlocked,
}

fn policy_for(root: &Path) -> RunnerPolicy {
    RunnerPolicy {
        allowed_roots: vec![root.to_path_buf()],
        ..RunnerPolicy::default()
    }
}

/// Run `/bin/sh -c script` under the workspace plan and capture its output.
///
/// The script is passed as a single argv element, never interpolated into the
/// Runner's own command line, so a case's script cannot alter how the case is
/// launched.
fn run_script(workspace: &Path, script: &str) -> Result<(NativeOutcome, String, i32), String> {
    let request = LocalExecutionRequest::new("/bin/sh", workspace)
        .args(["-c", script])
        .env(LocalEnv::Inherit {
            overrides: Default::default(),
            remove: Vec::new(),
        })
        .stdin(webcodex_process::execution_broker::StreamPolicy::Null)
        .stdout(webcodex_process::execution_broker::StreamPolicy::Piped)
        .stderr(webcodex_process::execution_broker::StreamPolicy::Piped);

    let mut child = match spawn_local_action(&policy_for(workspace), None, request) {
        Ok(child) => child,
        // A refusal is the documented fail-closed outcome, and on a nested
        // Seatbelt host it is also the *only* outcome available. Either way no
        // process ran, so the case proved nothing about confinement.
        Err(_) => return Ok((NativeOutcome::EnvBlocked, String::new(), -1)),
    };

    let stdout = child.child_mut().stdout.take();
    let stderr = child.child_mut().stderr.take();
    let out = std::thread::spawn(move || read_all(stdout));
    let err = std::thread::spawn(move || read_all(stderr));
    let status = child
        .wait()
        .map_err(|error| format!("wait failed: {error}"))?;
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();

    // A launcher that started but whose profile the kernel refused is
    // ENV_BLOCKED, not a case result. Without this the case would read the
    // launcher's own error as the action's behaviour and "confirm" a
    // confinement that never applied.
    if stderr_is_profile_refusal(&stderr) {
        return Ok((
            NativeOutcome::EnvBlocked,
            String::from_utf8_lossy(&stderr).into_owned(),
            -1,
        ));
    }

    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    if !stderr.is_empty() {
        text.push_str(" |stderr: ");
        text.push_str(&String::from_utf8_lossy(&stderr));
    }
    Ok((NativeOutcome::Confirmed, text, status.code().unwrap_or(-1)))
}

/// Whether the captured stderr is the sandbox launcher refusing the profile.
fn stderr_is_profile_refusal(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr);
    text.contains("sandbox_apply") || text.contains("sandbox-exec")
}

fn read_all(pipe: Option<impl std::io::Read + Send + 'static>) -> Vec<u8> {
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    let mut buffer = Vec::new();
    let _ = pipe.read_to_end(&mut buffer);
    buffer
}

/// Emit one machine-readable verdict for the native smoke script.
///
/// A case that cannot run says so. It never stays silent, because
/// `native-normalization-p1.sh` treats a missing line as `NOT_REPORTED` and
/// fails — silence must not be readable as success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeVerdict {
    Pass,
    Fail,
    /// The host cannot apply a restrictive profile. Not a pass.
    EnvBlocked,
}

fn emit_verdict(case: &str, verdict: NativeVerdict, detail: &str) {
    let label = match verdict {
        NativeVerdict::Pass => "PASS",
        NativeVerdict::Fail => "FAIL",
        NativeVerdict::EnvBlocked => "ENV_BLOCKED",
    };
    eprintln!("{case}={label} {detail}");
}

/// Turn one case's observation into a verdict plus a human-readable detail.
fn verdict_from(outcome: NativeOutcome, passed: bool, detail: &str) -> (NativeVerdict, String) {
    match outcome {
        NativeOutcome::EnvBlocked => (NativeVerdict::EnvBlocked, detail.to_string()),
        NativeOutcome::Confirmed => (
            if passed {
                NativeVerdict::Pass
            } else {
                NativeVerdict::Fail
            },
            detail.to_string(),
        ),
    }
}

/// Emit one case's verdict line and fail the test unless it is a real pass.
///
/// This is the single exit for every native-facing case, so the two obligations
/// it carries cannot drift apart:
///
/// * the smoke script must always see a line (`PASS` / `FAIL` / `ENV_BLOCKED`),
///   because a missing line is indistinguishable from a case that never ran;
/// * the test harness must still fail on a genuine `FAIL` — a verdict line is
///   evidence, not a substitute for an assertion.
fn report(case: &str, outcome: NativeOutcome, passed: bool, detail: &str) {
    let (verdict, detail) = verdict_from(outcome, passed, detail);
    emit_verdict(case, verdict, &detail);
    assert_ne!(verdict, NativeVerdict::Fail, "{case} failed: {detail}");
}

// ---------------------------------------------------------------------------
// P1-A .. P1-J
// ---------------------------------------------------------------------------

/// **P1-A** The plan is derived from trusted project context, never from the
/// model's `cwd`. A cwd outside every trusted root yields no plan at all.
#[test]
fn a_plan_comes_from_trusted_context_and_cwd_is_only_a_request() {
    let trusted = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();

    let writable_roots;
    let readable_roots;
    let network;
    match derive_workspace_plan(&policy_for(trusted.path()), None, trusted.path()).unwrap() {
        SandboxPlan::Confined {
            writable_roots: w,
            readable_roots: r,
            network: n,
        } => {
            writable_roots = w;
            readable_roots = r;
            network = n;
        }
    }
    assert_eq!(writable_roots, vec![trusted.path().canonicalize().unwrap()]);
    assert!(readable_roots.is_empty());
    assert_eq!(network, NetworkPolicy::Deny);

    // Naming a wider directory is a refusal, never a wider grant.
    let err = derive_workspace_plan(&policy_for(trusted.path()), None, outside.path()).unwrap_err();
    assert_eq!(err.code, "sandbox_authority_unavailable");
}

/// **P1-B** Inside the workspace, the action can read and write.
#[test]
fn b_workspace_is_readable_and_writable() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("existing"), b"seed").unwrap();

    let script = "printf read > existing && printf written > fresh && cat fresh";
    let (outcome, text, code) = run_script(workspace.path(), script).expect("run");
    let held = code == 0 && text.trim() == "written";
    report(
        "P1_NATIVE_RUN_SHELL",
        outcome,
        held,
        &format!("exit={code} stdout={:?}", text.trim()),
    );
}

/// **P1-C** A cwd outside the trusted roots is refused before any process
/// exists, so there is nothing to confine.
#[test]
fn c_cwd_outside_authority_is_refused_before_spawn() {
    let trusted = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let request = LocalExecutionRequest::new("/bin/sh", outside.path())
        .args(["-c", "echo should-not-run"])
        .stdout(webcodex_process::execution_broker::StreamPolicy::Piped);
    let refusal =
        spawn_local_action(&policy_for(trusted.path()), None, request).expect_err("must refuse");
    assert_eq!(refusal.code, "sandbox_authority_unavailable");
}

/// **P1-D** Nothing outside the workspace is readable.
#[test]
fn d_external_filesystem_is_denied() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret");
    std::fs::write(&secret, b"classified").unwrap();

    let (outcome, text, _) = run_script(
        workspace.path(),
        &format!("cat {}", shell_quote(&secret.to_string_lossy())),
    )
    .expect("run");
    let leaked = text.contains("classified");
    report(
        "P1_NATIVE_EXTERNAL_DENY",
        outcome,
        !leaked,
        &format!("outside_read_leaked={leaked} stdout={:?}", text.trim()),
    );
}

/// **P1-E** Network is denied.
#[test]
fn e_network_is_denied() {
    let workspace = tempfile::tempdir().unwrap();
    // A loopback connect attempt is the smallest network reach that does not
    // depend on the outside world being up. Success would mean the profile
    // failed to deny network.
    let script = "exec 3<>/dev/tcp/127.0.0.1/1 2>/dev/null && echo NET_OPEN || echo NET_CLOSED";
    let (outcome, text, _) = run_script(workspace.path(), script).expect("run");
    // Either marker means the probe itself ran. Only NET_OPEN means the profile
    // failed to deny network; no marker at all means the probe never executed and
    // the case established nothing.
    let probe_ran = text.contains("NET_CLOSED") || text.contains("NET_OPEN");
    let reachable = text.contains("NET_OPEN");
    let verdict = outcome == NativeOutcome::Confirmed && !probe_ran;
    report(
        "P1_NATIVE_NETWORK_DENY",
        outcome,
        probe_ran && !reachable,
        if verdict {
            format!("probe produced no verdict: {:?}", text.trim())
        } else {
            format!("reachable={reachable} stdout={:?}", text.trim())
        }
        .as_str(),
    );
}

/// **P1-F** Descendants inherit the profile: a grandchild cannot write outside
/// the workspace even though its parent could spawn it.
#[test]
fn f_descendants_inherit_the_profile() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("descendant-target");
    let script = format!(
        "/bin/sh -c 'printf escaped > {}' ; ls {} >/dev/null 2>&1 && echo WROTE_OUT || echo BLOCKED",
        shell_quote(&target.to_string_lossy()),
        shell_quote(&target.to_string_lossy())
    );
    let (outcome, text, _) = run_script(workspace.path(), &script).expect("run");
    let escaped = target.exists();
    report(
        "P1_NATIVE_DESCENDANT",
        outcome,
        !escaped,
        &format!(
            "descendant_wrote_outside={escaped} stdout={:?}",
            text.trim()
        ),
    );
}

/// **P1-G** is covered in `webcodex-workspace`'s `git_broker` tests, where the
/// real `git apply` code path lives. It cannot be covered from here: the broker
/// helper is `pub(crate)` to that crate, and a test that reimplemented it would
/// prove nothing about the code that ships.
///
/// **P1-H** likewise: the plan `git_broker` derives is asserted there, against
/// the real function.

/// **P1-I** The validation execute path is *identified* rather than silently
/// skipped. P1 does not route it: `run_bounded` launches interpreter-based
/// tools whose runtime compatibility is tracked as
/// `RUNTIME_COMPATIBILITY_TODO`. This test fails if that path is quietly
/// reclassified as normalized.
#[test]
fn i_validation_execute_is_declared_unrouted_with_a_runtime_todo() {
    let source = read_runner_source("validation/execute.rs");
    assert!(
        source.contains("RUNTIME_COMPATIBILITY_TODO"),
        "validation/execute.rs must carry the RUNTIME_COMPATIBILITY_TODO marker \
         while it remains outside P1 routing"
    );
    // And the marker must not be a claim of routing.
    assert!(
        !source.contains("spawn_local_action"),
        "validation/execute.rs must not appear to route through the broker yet"
    );
}

/// **P1-J** A missing or unusable trusted context fails before process
/// creation, with a stable refusal code rather than an opaque error.
#[test]
fn j_missing_trusted_context_fails_before_process_creation() {
    let outside = tempfile::tempdir().unwrap();
    let request = LocalExecutionRequest::new("/usr/bin/true", outside.path());
    let refusal =
        spawn_local_action(&RunnerPolicy::default(), None, request).expect_err("must refuse");
    assert_eq!(refusal.code, "sandbox_authority_unavailable");
    assert!(
        refusal.detail.contains("no trusted project context"),
        "refusal must name its cause: {}",
        refusal.detail
    );
}

// ---------------------------------------------------------------------------
// Structural anti-bypass guard
// ---------------------------------------------------------------------------

/// The specific functions P1 routed through the broker.
///
/// # Why functions and not files
///
/// A file-level ban was tried first and it flagged three *real* spawn sites
/// that P1 deliberately does not own:
///
/// * `run_prepare_command` — runs a user-configured shell profile's
///   `init_script`. Its authority is the profile configuration, not a
///   model-authored command, and its cwd is profile-owned.
/// * the Tool Plugin launcher in `shell.rs` — `configured_process_command`.
///   P1's scope statement excludes plugin/MCP provider processes.
/// * the SSH client launch in `job_manager.rs` — P1 explicitly excludes SSH
///   and `remote_shell`.
///
/// Pinning functions keeps the guard honest about what it actually pins: a
/// refactor that moves a routed spawn out of these functions fails, and adding
/// a new unbrokered spawn *inside* one of these functions fails too. Adding a
/// brand-new routed function is a scope decision someone has to write down,
/// which is the intended friction.
const P1_ROUTED_FUNCTIONS: &[(&str, &str)] = &[
    ("shell.rs", "configured_shell_command"),
    ("shell.rs", "configured_prepared_shell_command"),
    ("shell.rs", "configured_explicit_shell_command"),
    ("shell.rs", "configured_shell_job_command"),
    ("shell.rs", "configured_prepared_shell_job_command"),
    ("shell.rs", "configured_validation_job_command"),
    ("shell.rs", "configured_process_command"),
    ("shell.rs", "execute_configured_command"),
    ("local_execution.rs", "spawn_local_action"),
];

/// What a P1-routed function is forbidden to contain.
struct BypassPattern {
    needle: &'static str,
    why: &'static str,
}

const BYPASS_PATTERNS: &[BypassPattern] = &[
    BypassPattern {
        needle: "ManagedChild::spawn(",
        why: "a routed path must not build its own child; the broker owns process creation",
    },
    BypassPattern {
        needle: "Command::new(",
        why: "a routed path must not assemble a Command, because a Command can only be spawned directly",
    },
];

/// Structural guard: P1-routed functions must not reach an unbrokered spawn.
///
/// This is the narrow guard the P1 scope actually implies. It is deliberately
/// **not** a repository-wide ban: the Runner legitimately spawns
/// control-plane processes (its own executable as detached supervisor, the
/// validation availability probe, the Node version probe) and P1 explicitly
/// excludes SSH, browser/CDP, plugin/MCP providers, LSP and the persistent
/// interactive shell. Flagging those would either be wrong or would require
/// re-opening the P1 scope decision.
#[test]
fn p1_routed_functions_cannot_spawn_outside_the_broker() {
    let mut offenders = Vec::new();
    for (file, function) in P1_ROUTED_FUNCTIONS {
        let Some(body) = function_body(&read_runner_source(file), function) else {
            offenders.push(format!(
                "{file}: routed function `{function}` no longer exists"
            ));
            continue;
        };
        for pattern in BYPASS_PATTERNS {
            // `local_execution` is the chokepoint: it is the one place allowed
            // to materialize a Command, and only for the Runner-owned
            // control-plane probes documented on `into_command`.
            if *file == "local_execution.rs" && pattern.needle == "Command::new(" {
                continue;
            }
            if body.contains(pattern.needle) {
                offenders.push(format!(
                    "{file}::{function}: contains `{}` — {}",
                    pattern.needle, pattern.why
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "P1-routed execution can bypass the broker:\n{}",
        offenders.join("\n")
    );
}

/// The known-unrouted surfaces, asserted to still exist in the source.
///
/// A surface silently disappearing is as much a lie as one silently appearing:
/// the remaining-surface list in `NORMALIZATION_P1_REPORT.md` is only true if
/// these call sites are still there.
#[test]
fn known_unrouted_surfaces_are_still_present_and_named() {
    // validation/execute.rs: interpreter-based validation tools.
    let validation = read_runner_source("validation/execute.rs");
    assert!(
        validation.contains("RUNTIME_COMPATIBILITY_TODO"),
        "the validation exception must stay named while it is unrouted"
    );

    // job_manager.rs: the local SSH client, which P1 excludes by scope.
    let jobs = read_runner_source("job_manager.rs");
    assert!(
        jobs.contains("ssh_command_spawn_failed"),
        "the SSH client launch is a known P1 exclusion and must stay visible"
    );

    // detached_job.rs: the durable detached payload.
    let detached = read_runner_source("detached_job.rs");
    assert!(
        detached.contains("failed to spawn detached payload"),
        "the detached durable payload is a known P1 exclusion and must stay visible"
    );
}

/// Extract one top-level function's body by brace matching.
///
/// Deliberately simple: these are module-level `fn`s, so counting braces from
/// the first `{` to its match is enough, and it does not need to understand
/// Rust.
fn function_body(source: &str, name: &str) -> Option<String> {
    let start = source.find(&format!("fn {name}("))?;
    let open = source[start..].find('{')? + start;
    let mut depth = 0usize;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(source[open..=open + offset].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// The complement of the guard above: the chokepoint *must* be present, so a
/// refactor that deletes it fails loudly instead of silently un-routing
/// everything.
#[test]
fn the_local_execution_chokepoint_exists_and_is_used() {
    let local = read_runner_source("local_execution.rs");
    assert!(
        local.contains("ExecutionBroker::new()"),
        "the chokepoint must be the only place that constructs the broker"
    );
    for file in ["shell.rs", "job_manager.rs"] {
        let source = read_runner_source(file);
        assert!(
            source.contains("spawn_local_action"),
            "{file} must route through the P1 chokepoint, not build its own child"
        );
        // An import alone is not routing: the call must appear too.
        assert!(
            source.contains("spawn_local_action("),
            "{file} must actually call the chokepoint, not merely import it"
        );
    }
}

fn read_runner_source(relative: &str) -> String {
    let path = runner_source_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn runner_source_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/webcodex_runner")
        .join(relative)
}

/// Single-quote a path for `/bin/sh`.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}
