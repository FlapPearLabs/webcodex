// SPDX-License-Identifier: Apache-2.0
//! P1 functional coverage: cases A-J, plus the legacy P1-only structural checks.
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
//! * **The legacy structural checks** pin selected P1 call sites and the
//!   project-git exception. They are not the complete production-process
//!   inventory; the independent `webcodex-process` P1B guard owns that boundary.
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
//! remaining-surface list cannot quietly become wrong. Complete anti-bypass
//! coverage across production targets is enforced by the independent P1B guard.

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use super::config::{RunnerPolicy, ShellConfig, ShellEnvironmentMode};
use super::local_execution::{
    approved_inherited_env, derive_workspace_plan, spawn_local_action, LocalExecutionRequest,
};
use super::shell::{run_shell_with_profiles_and_execution_state, PreparedShellProfileCache};
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

/// A registry + policy pair for one workspace, as production reads them.
///
/// # Why every case needs a registered project
///
/// Authority is registered project context and nothing else (P1 closure). A case
/// that passes `allowed_roots` and no registry is no longer testing "the plan
/// confines to the workspace" — it is testing the refusal path, and it would
/// keep reporting a confinement that never applied. So the harness makes the
/// registration explicit and each case gets a real project.
struct Project {
    /// Directory holding the registered project root.
    workspace: tempfile::TempDir,
    /// Directory holding the registry. Kept alongside `workspace` so both live
    /// as long as the struct.
    _keeper: tempfile::TempDir,
    /// Registry directory to hand to the production entry points.
    registry: PathBuf,
    /// The registered project root.
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let keeper = tempfile::tempdir().unwrap();
        let registry = keeper.path().join("registry");
        std::fs::create_dir_all(&registry).unwrap();
        let root = workspace.path().canonicalize().unwrap();
        std::fs::write(
            registry.join("p1-project.toml"),
            format!("id = \"p1-project\"\npath = {:?}\n", root.to_string_lossy()),
        )
        .unwrap();
        Self {
            workspace,
            _keeper: keeper,
            registry,
            root,
        }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn registry(&self) -> &Path {
        &self.registry
    }

    /// The policy production would hold. `allowed_roots` is still carried —
    /// for file operations — but authority no longer comes from it, so these
    /// cases deliberately keep it present to prove that it confers nothing.
    fn policy(&self) -> RunnerPolicy {
        RunnerPolicy {
            allowed_roots: vec![self.root.clone()],
            ..RunnerPolicy::default()
        }
    }
}

/// A shell configuration for the native cases.
///
/// `Inherit` is the interesting mode: it is the one that used to hand the
/// Runner's whole environment to the child, so running the native path in this
/// mode is what makes the environment assertions mean anything. The program is
/// pinned to `/bin/sh` so a case measures confinement, not dialect resolution.
fn shell_config() -> ShellConfig {
    ShellConfig {
        environment_mode: ShellEnvironmentMode::Inherit,
        program: "/bin/sh".to_string(),
        ..ShellConfig::default()
    }
}

/// Run `script` through the **real** production shell entry point.
///
/// # Which layer this enters
///
/// [`run_shell_with_profiles_and_execution_state`] is the function the Runner's
/// own request path calls. From here the chain under test is the production
/// chain, end to end:
///
/// ```text
/// run_shell_with_profiles_and_execution_state
///   -> run_shell_impl
///     -> configured_shell_command          (command text + environment rule)
///     -> execute_configured_command
///       -> spawn_local_action             (the chokepoint)
///         -> resolve_workspace_authority  (registered project only)
///         -> ExecutionBroker::spawn_with_toolchain
///           -> Codex-derived Seatbelt profile
///             -> task process tree
/// ```
///
/// Nothing here re-implements any part of that. The previous version of this
/// file called [`spawn_local_action`] directly, which proved the chokepoint works
/// but proved nothing about whether production reaches it: a `run_shell` that
/// stopped routing would have left every case here green.
///
/// `Err` is reserved for "the harness itself could not run the case". A
/// production refusal is not that — it is reported as
/// [`NativeOutcome::EnvBlocked`], because on a nested Seatbelt host refusing is
/// the only correct outcome available.
fn run_production_shell(
    project: &Project,
    script: &str,
) -> Result<(NativeOutcome, String, i32), String> {
    let cache = PreparedShellProfileCache::default();
    let cwd = project.path().to_string_lossy().into_owned();
    let result = run_shell_with_profiles_and_execution_state(
        0,
        &project.policy(),
        &shell_config(),
        project.registry(),
        &cache,
        Some(&cwd),
        script,
        None,
        false,
        None,
        30,
        None,
    );

    let stdout = result.result.stdout.clone().unwrap_or_default();
    let stderr = result.result.stderr.clone().unwrap_or_default();
    let error = result.result.error.clone().unwrap_or_default();
    let combined = format!("{stdout}\n{stderr}\n{error}");

    // Three shapes all mean "no process ran", which is ENV_BLOCKED rather than a
    // case result:
    //   * the launcher started and the kernel refused the profile,
    //   * the broker refused and `run_shell` recorded an error,
    //   * `run_shell` refused before spawning (fail-closed).
    // Reading any of them as the action's behaviour would "confirm" a
    // confinement that never applied.
    if is_sandbox_refusal_text(&combined) {
        return Ok((NativeOutcome::EnvBlocked, combined, -1));
    }
    if result.execution_state == crate::runner_protocol::ShellCommandExecutionState::NotStarted {
        return Ok((NativeOutcome::EnvBlocked, combined, -1));
    }

    Ok((
        NativeOutcome::Confirmed,
        combined,
        result.result.exit_code.unwrap_or(-1),
    ))
}

/// Whether the captured text shows the sandbox launcher refusing the profile.
fn is_sandbox_refusal_text(text: &str) -> bool {
    text.contains("sandbox_apply") || text.contains("sandbox-exec")
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
    assert_eq!(
        verdict,
        NativeVerdict::Pass,
        "{case} did not pass on this host: {detail}"
    );
}

#[test]
fn native_verdict_gate_rejects_env_blocked() {
    let rejected = std::panic::catch_unwind(|| {
        report(
            "P1_NATIVE_ACCOUNTING_ONLY",
            NativeOutcome::EnvBlocked,
            true,
            "host cannot apply the restrictive profile",
        )
    });
    assert!(rejected.is_err(), "ENV_BLOCKED must fail native accounting");
}

/// Drop every `#[cfg(test)]` module from a source file.
///
/// The anti-bypass guards are claims about **production** code. A test module
/// legitimately does things production must not: compile a fixture with
/// `rustc`, spawn a helper binary directly to exercise a cleanup routine in
/// isolation. Asserting over the raw file would flag those and force the guard
/// to be weakened until it no longer guarded anything.
///
/// `#[cfg(test)]` is an unambiguous production/non-production marker, so
/// removing exactly those modules is sound: it cannot hide production code, and
/// it cannot be arranged to move a production item inside one.
fn production_region(source: &str) -> String {
    let mut kept = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let rest = &source[index..];
        let is_cfg_test = rest.starts_with("#[cfg(test)]");
        // Find the `mod <name> {` that the attribute applies to.
        let body_start = rest.find('{').filter(|offset| {
            // Everything between the attribute and the brace must be the item
            // header (`mod tests`, `mod tests;`, visibility, attributes).
            let header = &rest[..*offset];
            header.contains("mod ") && !header.contains('}') && !header.contains(';')
        });
        match (is_cfg_test, body_start) {
            (true, Some(offset)) => {
                let open = index + offset;
                let mut depth = 0usize;
                let mut cursor = open;
                for (delta, ch) in source[open..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                cursor = open + delta + ch.len_utf8();
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                index = cursor;
            }
            _ => {
                let ch_len = source[index..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
                kept.push_str(&source[index..index + ch_len]);
                index += ch_len;
            }
        }
    }
    kept
}

/// Assert `needle` appears in `source` only inside line comments.
///
/// A doc comment that names a forbidden construct to explain what was removed
/// is documentation, not a call site. Matching the raw substring would make it
/// impossible to *explain* the fix, which is how these guards decay into
/// silence. This walks line by line and requires every occurrence to sit on a
/// line whose first non-space character is `//`, so a real usage — which by
/// definition is code — still fails.
fn occurrences_are_only_in_line_comments(source: &str, needle: &str) -> bool {
    source
        .lines()
        .filter(|line| line.contains(needle))
        .all(|line| line.trim_start().starts_with("//"))
}

// ---------------------------------------------------------------------------
// P1-A .. P1-M
// ---------------------------------------------------------------------------

/// **P1-A** The plan is derived from registered project context, never from the
/// model's `cwd`. A cwd no registered project covers yields no plan at all.
#[test]
fn a_plan_comes_from_trusted_context_and_cwd_is_only_a_request() {
    let project = Project::new();
    let outside = tempfile::tempdir().unwrap();

    let writable_roots;
    let readable_roots;
    let network;
    match derive_workspace_plan(Some(project.registry()), project.path()).unwrap() {
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
    assert_eq!(writable_roots, vec![project.root.clone()]);
    assert!(readable_roots.is_empty());
    assert_eq!(network, NetworkPolicy::Deny);

    // Naming a wider directory is a refusal, never a wider grant.
    let err =
        derive_workspace_plan(Some(project.registry()), outside.path()).expect_err("must refuse");
    assert_eq!(err.code, "sandbox_authority_unavailable");
}

/// **P1-B** Inside the workspace, the action can read and write.
///
/// Enters through the real `run_shell` boundary: the file was written by the
/// same call chain production uses, not by a test that reached the broker
/// directly.
#[test]
fn b_workspace_is_readable_and_writable() {
    let project = Project::new();
    std::fs::write(project.path().join("existing"), b"seed").unwrap();

    let script = "printf read > existing && printf written > fresh && cat fresh";
    let (outcome, text, code) = run_production_shell(&project, script).expect("run");
    let held = code == 0 && text.trim().contains("written");
    report(
        "P1_NATIVE_RUN_SHELL",
        outcome,
        held,
        &format!("exit={code} stdout={:?}", text.trim()),
    );
}

/// **P1-C** A cwd outside the registered projects is refused before any process
/// exists, so there is nothing to confine.
///
/// This one is about the chokepoint's own precondition, so it calls the
/// chokepoint: `run_shell` funnels a bad cwd into the same refusal, and P1-H
/// below covers the production side of that.
#[test]
fn c_cwd_outside_authority_is_refused_before_spawn() {
    let project = Project::new();
    let outside = tempfile::tempdir().unwrap();
    let request = LocalExecutionRequest::new("/bin/sh", outside.path())
        .args(["-c", "echo should-not-run"])
        .stdout(webcodex_process::execution_broker::StreamPolicy::Piped);
    let refusal = spawn_local_action(Some(project.registry()), request).expect_err("must refuse");
    assert_eq!(refusal.code, "sandbox_authority_unavailable");
}

/// **P1-D** Nothing outside the workspace is readable, through the real
/// `run_shell` boundary.
#[test]
fn d_external_filesystem_is_denied() {
    let project = Project::new();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret");
    std::fs::write(&secret, b"classified").unwrap();

    let (outcome, text, _) = run_production_shell(
        &project,
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

/// **P1-E** Network is denied, proven with a positive control.
///
/// # Why the old version of this case was worthless
///
/// It ran `exec 3<>/dev/tcp/127.0.0.1/1` and asserted the connect failed.
/// Port 1 has nothing listening, so the connect fails **whether or not a
/// sandbox is present**. The case passed on a host with no confinement at all —
/// it could not distinguish "the profile denied network" from "the port is
/// closed".
///
/// # What replaces it
///
/// A listener this test process owns, on a port it chose:
///
/// 1. bind `127.0.0.1:<port>` in the test process — `LISTENER_BOUND`;
/// 2. connect to it from an **unsandboxed** child — `UNSANDBOXED_CONNECT_PASS`;
/// 3. prove the listener is *still* listening — `LISTENER_STILL_LIVE_BEFORE_SANDBOX`;
/// 4. only then connect from the **production** `run_shell` path —
///    `SANDBOX_PROCESS_STARTED`, then `SANDBOX_CONNECT_DENIED`.
///
/// A PASS requires all five.
///
/// # Why the listener must be owned by the parent
///
/// An earlier version moved the `TcpListener` into a thread that accepted the
/// unsandboxed control connection and then exited. That dropped the listener
/// **before** the sandboxed measurement, so the sandboxed connect hit a closed
/// port — and an entirely unconfined process would also have been refused. The
/// case therefore false-passed: it could not tell "the profile denied network"
/// from "nothing is listening any more".
///
/// So the listener stays in the parent, and step 3 re-probes liveness with a
/// second unsandboxed connect immediately before the sandboxed attempt. If the
/// listener were dropped at any point, that probe fails and the case reports
/// `TEST_BLOCKED` instead of a vacuous PASS.
#[test]
fn e_network_is_denied_with_a_positive_control() {
    let project = Project::new();
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            report(
                "P1_NATIVE_NETWORK_DENY",
                NativeOutcome::Confirmed,
                false,
                &format!("LISTENER_BOUND=false bind failed: {error}"),
            );
            return;
        }
    };
    let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
    if port == 0 {
        report(
            "P1_NATIVE_NETWORK_DENY",
            NativeOutcome::Confirmed,
            false,
            "LISTENER_BOUND=false no port assigned",
        );
        return;
    }

    // Phase A: the unsandboxed positive control. The listener is borrowed, not
    // moved, so it stays owned by this frame and remains open afterwards.
    if let Some(reason) = unsandboxed_connect_fails(port, "UNSANDBOXED_CONNECT_PASS") {
        report(
            "P1_NATIVE_NETWORK_DENY",
            NativeOutcome::Confirmed,
            false,
            &format!("UNSANDBOXED_CONNECT_PASS=false {reason}"),
        );
        return;
    }

    // Phase B: prove the listener is still live. This is the regression guard for
    // the dropped-listener bug: if the socket were closed by now, this probe
    // fails and the case stops rather than reporting a vacuous denial.
    //
    // The probe connection is drained by a short-lived accept on the borrowed
    // listener, so the listen backlog is not left holding a pending connection.
    if let Err(error) = listener.set_nonblocking(true) {
        report(
            "P1_NATIVE_NETWORK_DENY",
            NativeOutcome::Confirmed,
            false,
            &format!("LISTENER_STILL_LIVE_BEFORE_SANDBOX=false cannot probe listener: {error}"),
        );
        return;
    }
    let still_live =
        unsandboxed_connect_fails(port, "LISTENER_STILL_LIVE_BEFORE_SANDBOX").is_none();
    // Accept whatever the liveness probe queued, so the measurement below faces
    // a listener with an empty backlog.
    let _ = listener.accept();
    if !still_live {
        report(
            "P1_NATIVE_NETWORK_DENY",
            NativeOutcome::Confirmed,
            false,
            &format!(
                "LISTENER_STILL_LIVE_BEFORE_SANDBOX=false the listener stopped accepting between \
                 the positive control and the sandboxed measurement on port {port}; a sandboxed \
                 refusal would prove nothing"
            ),
        );
        return;
    }

    // Phase C: the same connect, through the production path. It must be
    // refused, and it must NOT be accepted by the listener — a sandboxed
    // process that reached the listener would mean the profile leaked.
    let sandbox_script = format!(
        "exec 3<>/dev/tcp/127.0.0.1/{port} 2>/dev/null && echo SANDBOX_CONNECTED || \
         echo SANDBOX_REFUSED"
    );
    let (outcome, text, _) = run_production_shell(&project, &sandbox_script).expect("run");
    let reachable = text.contains("SANDBOX_CONNECTED");
    let refused = text.contains("SANDBOX_REFUSED");

    // The listener must still be live *after* the sandboxed attempt too. If the
    // sandboxed child had reached it, the socket would have accepted a second
    // connection and this probe would find nothing to accept.
    let listener_survived =
        unsandboxed_connect_fails(port, "LISTENER_SURVIVED_SANDBOX_ATTEMPT").is_none();
    let _ = listener.accept();

    report(
        "P1_NATIVE_NETWORK_DENY",
        outcome,
        refused && !reachable && listener_survived,
        &format!(
            "LISTENER_BOUND=true UNSANDBOXED_CONNECT_PASS=true \
             LISTENER_STILL_LIVE_BEFORE_SANDBOX={still_live} SANDBOX_PROCESS_STARTED={} \
             SANDBOX_CONNECT_DENIED={refused} LISTENER_SURVIVED_SANDBOX_ATTEMPT={listener_survived} \
             reachable={reachable} stdout={:?}",
            matches!(outcome, NativeOutcome::Confirmed),
            text.trim()
        ),
    );
}

/// Connect to `127.0.0.1:<port>` from an **unsandboxed** `/bin/sh` child.
///
/// Returns `Err(reason)` when the child could not be spawned or the connect did
/// not succeed. Used both as the positive control and as the liveness probe
/// around the sandboxed measurement, so the two phases are proven against the
/// same live socket rather than against a remembered port number.
fn unsandboxed_connect_fails(port: u16, label: &str) -> Option<String> {
    let script = format!(
        "exec 3<>/dev/tcp/127.0.0.1/{port} 2>/dev/null && echo CONTROL_CONNECTED || echo CONTROL_REFUSED"
    );
    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    match output {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            if text.contains("CONTROL_CONNECTED") {
                None
            } else {
                Some(format!(
                    "{label}=false an unsandboxed child could not reach the listener on port \
                     {port}: {:?}",
                    text.trim()
                ))
            }
        }
        Err(error) => Some(format!("{label}=false control spawn failed: {error}")),
    }
}

/// **Regression guard for the dropped-listener false-pass.** The network case
/// trusts `LISTENER_STILL_LIVE_BEFORE_SANDBOX` to distinguish "the profile denied
/// network" from "nothing is listening any more". That trust is only sound if the
/// liveness probe actually *fails* on a closed port. This pins both directions on
/// a real socket, with no sandbox involved, so the guard cannot rot into a
/// tautology:
///
/// * a live listener is reachable, so the probe reports no failure;
/// * the same port, after the listener is dropped, is refused, so the probe
///   reports a failure — which is exactly what makes case E stop instead of
///   printing a vacuous `SANDBOX_CONNECT_DENIED=true`.
///
/// Without the second half, an earlier build that moved the listener into a
/// short-lived accept thread passed this gate: the port was closed, every
/// connect was refused, and the denial looked like proof.
#[test]
fn liveness_probe_distinguishes_a_live_listener_from_a_dropped_one() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a loopback port");
    let port = listener.local_addr().expect("local addr").port();
    assert_ne!(port, 0, "bind must assign a real port");

    // Direction 1: while the listener is owned by this frame, the unsandboxed
    // probe reaches it, so the helper reports no failure.
    assert!(
        unsandboxed_connect_fails(port, "LIVE_LISTENER").is_none(),
        "a listener this process still owns must be reachable, otherwise the \
         positive control in case E can never pass"
    );

    // Direction 2: drop it. The very same probe on the very same port must now
    // report a failure — this is the condition case E has to catch.
    drop(listener);
    let refusal = unsandboxed_connect_fails(port, "DROPPED_LISTENER")
        .expect("a dropped listener must be reported as unreachable");
    assert!(
        refusal.contains("DROPPED_LISTENER=false"),
        "the failure reason must name the probe that failed, got: {refusal}"
    );
}

/// **P1-F** Descendants inherit the profile: a grandchild cannot write outside
/// the workspace even though its parent could spawn it. Through the real
/// `run_shell` boundary.
#[test]
fn f_descendants_inherit_the_profile() {
    let project = Project::new();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("descendant-target");
    let script = format!(
        "/bin/sh -c 'printf escaped > {}' ; ls {} >/dev/null 2>&1 && echo WROTE_OUT || echo BLOCKED",
        shell_quote(&target.to_string_lossy()),
        shell_quote(&target.to_string_lossy())
    );
    let (outcome, text, _) = run_production_shell(&project, &script).expect("run");
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

/// **P1-G** (`git apply` through the real `workspace_checkpoint` wrapper) and
/// **P1-H** (the plan `git_broker` derives) are covered in `webcodex-workspace`,
/// where that code lives. They cannot be covered from here: the checkpoint
/// module and the broker helper are `pub(crate)` to that crate, and a test that
/// reimplemented them would prove nothing about the code that ships.

/// **P1-I** The validation execute path is now **routed** through the broker.
///
/// The previous version of this file asserted the opposite — that
/// `validation/execute.rs` stayed unbrokered and carried a
/// `RUNTIME_COMPATIBILITY_TODO`. That was honest about P1's first round but it
/// was also the reason P1 could not be called complete: validation is a
/// model-triggered local execution surface, and "we know we skip it" is not the
/// same as "it is confined".
///
/// The route is now mandatory. If the interpreter cannot start under the
/// trusted runtime policy, `run_bounded` returns a spawn failure — it must never
/// fall back to a bare `Command::spawn`. Runtime compatibility and execution
/// normalization are now separate claims: the first may be `FAIL`, the second
/// must be `PASS`.
#[test]
fn i_validation_execute_is_brokered_and_cannot_direct_spawn() {
    let source = read_runner_source("validation/execute.rs");
    assert!(
        !source.contains("RUNTIME_COMPATIBILITY_TODO"),
        "validation/execute.rs must no longer claim to be un-routed; it is brokered now"
    );
    // The anti-bypass property, scoped to production code. The test module is
    // excluded because it legitimately compiles a fixture with `rustc` and
    // spawns a helper directly to exercise `terminate_validation_child` in
    // isolation — neither is a production execution path.
    let production = production_region(&source);
    for forbidden in ["Command::new(", "ManagedChild::spawn("] {
        assert!(
            !production.contains(forbidden),
            "validation/execute.rs must not contain `{forbidden}` outside its test module — \
             it must go through spawn_local_action, or a validation process can escape \
             the broker"
        );
    }
    assert!(
        production.contains("spawn_local_action"),
        "validation/execute.rs must route through the P1 chokepoint"
    );
    assert!(
        production.contains("request_terminate_tree"),
        "process-tree ownership and shutdown cleanup must survive the routing"
    );
}

/// **P1-K (closure)** A model-triggered action must not receive the Runner's
/// environment.
///
/// This is the P0 the first round got wrong: the old `LocalEnv::Inherit`
/// reached the broker as `EnvPolicy::Inherit`, which is the Runner's entire
/// environment minus a five-name credential denylist. The test uses names the
/// denylist never mentioned, because a denylist that only knows about the
/// credentials someone already thought of is not a boundary.
#[test]
fn k_model_triggered_env_is_positively_selected_not_inherited() {
    // A deliberately unknown variable: no denylist in the world lists this.
    let host: HashMap<String, String> = [
        ("TOTALLY_NEW_SECRET_123", "leaked"),
        ("GH_TOKEN", "ghp_leaked"),
        ("GITHUB_TOKEN", "ghp_leaked"),
        ("OPENAI_API_KEY", "sk-leaked"),
        ("AWS_SECRET_ACCESS_KEY", "leaked"),
        ("NPM_TOKEN", "leaked"),
        ("DESKTOP_MCP_SOURCE_TOKEN", "leaked"),
        // The approved ones, which must survive.
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "en_US.UTF-8"),
        ("TERM", "xterm-256color"),
        ("NO_COLOR", "1"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect();

    let selected = approved_inherited_env(&host);
    let names: Vec<&str> = selected.iter().map(|(key, _)| key.as_str()).collect();

    for leaked in [
        "TOTALLY_NEW_SECRET_123",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "OPENAI_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "NPM_TOKEN",
        "DESKTOP_MCP_SOURCE_TOKEN",
    ] {
        assert!(
            !names.contains(&leaked),
            "`{leaked}` reached a model-triggered environment; the approved set is a \
             positive list and must not contain it"
        );
    }
    for kept in ["PATH", "LANG", "TERM", "NO_COLOR"] {
        assert!(
            names.contains(&kept),
            "`{kept}` is an approved runtime variable and must be retained; got {names:?}"
        );
    }

    // And the variant that made this possible must not exist as code. Doc
    // comments naming it are allowed — see the comment rule below.
    let local = read_runner_source("local_execution.rs");
    assert!(
        occurrences_are_only_in_line_comments(&production_region(&local), "LocalEnv::Inherit"),
        "LocalEnv::Inherit must stay deleted as a variant: it reaches the broker as \
         EnvPolicy::Inherit, which hands the Runner's whole environment to the child"
    );

    // No P1 model-triggered surface may *request* `EnvPolicy::Inherit`. Comments
    // naming it are explicitly allowed — the closure has to be able to explain
    // what it removed, and a guard that forbade the explanation would push the
    // next reader to delete the comment instead of the code.
    for file in ["shell.rs", "local_execution.rs", "job_manager.rs"] {
        let production = production_region(&read_runner_source(file));
        assert!(
            occurrences_are_only_in_line_comments(&production, "EnvPolicy::Inherit"),
            "{file} contains a live `EnvPolicy::Inherit` request; no P1 model-triggered \
             execution may inherit the Runner's environment"
        );
    }
}

/// **P1-L (closure)** `HOME` is not an execution authority.
///
/// The runner config carries `$HOME` in `allowed_roots` as a generic "the user
/// said anywhere under here" grant — `effective_allowed_roots` *substitutes* it
/// whenever the operator configured nothing. For file operations that is still a
/// sensible default. For execution it hands every model-triggered action the
/// user's entire home directory, which is the opposite of confining it to a
/// project.
///
/// The closure removes the possibility structurally rather than by filtering:
/// `resolve_workspace_authority` no longer takes a policy at all, so `$HOME`
/// cannot reach it. These two cases pin the behaviour that remains.
///
/// * `HOME` trusted as a generic `allowed_root` but **no registered project**
///   covering the cwd → refused, not silently granted;
/// * a **registered project under `HOME`** → authority is the project root, not
///   `HOME`.
#[test]
fn l_home_is_not_a_project_authority() {
    // A stand-in for $HOME with a project inside it.
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();

    // HOME is trusted as a generic allowed_root, exactly as the config allows —
    // and deliberately kept in the policy even for the case that must refuse.
    let policy = RunnerPolicy {
        allowed_roots: vec![home.path().to_path_buf()],
        ..RunnerPolicy::default()
    };

    // No project registry: nothing establishes a *project* authority, so a
    // directory that is only covered by the coarse HOME grant must be refused
    // rather than inheriting the whole home directory. `allowed_roots` cannot
    // even be offered to the resolver any more, so the case states the refusal
    // through the public surface a caller has.
    let loose = home.path().join("not-a-project");
    std::fs::create_dir_all(&loose).unwrap();
    let empty_registry = home.path().join("registry");
    std::fs::create_dir_all(&empty_registry).unwrap();
    let err = derive_workspace_plan(Some(&empty_registry), &loose)
        .expect_err("HOME must not be authority");
    assert_eq!(
        err.code, "sandbox_authority_unavailable",
        "a coarse HOME grant must not become a project authority"
    );
    assert!(
        !policy.allowed_roots.is_empty(),
        "the HOME grant is still configured for file operations; it is simply not \
         reachable from execution"
    );

    // With a registered project, the authority is the project root — narrower
    // than HOME, which is the whole narrowest-wins point. The registry is read
    // as `<name>.toml` with `id` and `path`, so the test writes the same file
    // format production reads rather than calling a registration helper that
    // might diverge from it.
    let registry = home.path().join("registry");
    std::fs::create_dir_all(&registry).unwrap();
    std::fs::write(
        registry.join("project.toml"),
        format!(
            "id = \"closure-project\"\npath = {:?}\n",
            project.to_string_lossy()
        ),
    )
    .unwrap();
    let authority = super::sandbox_authority::resolve_workspace_authority(
        Some(&registry),
        &project.join("src"),
    )
    .expect("a registered project establishes authority");
    assert_eq!(
        authority.root(),
        project.canonicalize().unwrap(),
        "authority must be the project root, never the enclosing HOME"
    );
    assert_ne!(authority.root(), home.path().canonicalize().unwrap());

    // And the resolver has no parameter through which `$HOME` could be handed
    // to it. This is the structural half: it holds even if someone later adds a
    // second trusted source, because there is no slot to add it to.
    let resolver = read_runner_source("sandbox_authority.rs");
    let signature = function_body(&resolver, "resolve_workspace_authority")
        .expect("resolve_workspace_authority must exist");
    let declaration = resolver
        .split("fn resolve_workspace_authority(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap_or_default();
    assert!(
        !declaration.contains("policy") && !declaration.contains("allowed_roots"),
        "resolve_workspace_authority must not accept a policy: `allowed_roots` is the \
         vector that carries an implicit `$HOME`, so accepting it is how the fallback \
         returns. Declaration was: ({declaration})"
    );
    assert!(
        !signature.contains("policy.allowed_roots"),
        "the resolver body must never read policy.allowed_roots"
    );
}

/// **P1-M (closure)** `narrowest_covering` is order-independent.
///
/// It used to return the *first* covering root, which made the result depend on
/// the order the caller happened to supply. A caller that put a coarse root
/// first would confine a project to its parent. Narrowest must mean narrowest:
/// the deepest covering root wins regardless of the order it arrived in.
#[test]
fn m_narrowest_covering_is_order_independent() {
    let outer = tempfile::tempdir().unwrap();
    let inner = outer.path().join("project");
    std::fs::create_dir_all(inner.join("src")).unwrap();
    let candidate = inner.join("src");
    let expected = inner.canonicalize().unwrap();

    let outer_root = outer.path().to_path_buf();
    let inner_root = inner.clone();

    // Both orders, plus duplicates, must land on the project.
    for roots in [
        vec![outer_root.clone(), inner_root.clone()],
        vec![inner_root.clone(), outer_root.clone()],
        vec![outer_root.clone(), inner_root.clone(), outer_root.clone()],
    ] {
        let authority = webcodex_process::execution_broker::WorkspaceAuthority::narrowest_covering(
            &candidate, &roots,
        )
        .expect("covered");
        assert_eq!(
            authority.root(),
            expected,
            "narrowest_covering returned {:?} for order {:?}; it must be the deepest \
             covering root regardless of caller order",
            authority.root(),
            roots
        );
    }
}

/// **P1-J** A missing or unusable trusted context fails before process
/// creation, with a stable refusal code rather than an opaque error.
#[test]
fn j_missing_trusted_context_fails_before_process_creation() {
    let outside = tempfile::tempdir().unwrap();
    // A registry that exists but registers nothing: the shape of a Runner whose
    // operator never added the project this request asks for.
    let empty_registry = tempfile::tempdir().unwrap();
    let request = LocalExecutionRequest::new("/usr/bin/true", outside.path());
    let refusal =
        spawn_local_action(Some(empty_registry.path()), request).expect_err("must refuse");
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

/// The specific functions P1 routed through the broker (legacy subset only).
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

/// P1B: the complete, exhaustive allowlist of production callers of
/// `CommandBlueprint::into_command()` — the one escape hatch out of the broker.
///
/// This is an *enumeration with a count*, not a pattern. The rule the guard
/// above enforces is "a model-reachable path must not reach `into_command()`".
/// That rule is only meaningful if the set of legitimate callers is written
/// down and pinned, because otherwise the answer to "who bypasses the broker
/// here?" is discovered by reading the whole tree instead of by reading a
/// test.
///
/// Every entry is a **fixed Runner-owned control-plane probe** whose payload is
/// authored by the Runner, never by a model:
///
/// 1. `shell.rs::configured_script_runtime_plan` — a `node --version`
///    capability probe. argv is a constant.
/// 2. `main.rs::validation_module_available` — a `python -I -c <PROBE> <module>`
///    import probe. The probe body is a hardcoded constant; the module name comes
///    from a configured validation job's own args, not from a model tool call.
///
/// Both are `CONTROL_PLANE_FIXED_PROBE`. Neither is model-reachable, and the
/// count is asserted below so a third production caller cannot appear without
/// this list — and this list — being updated on purpose.
const P1B_ALLOWED_INTO_COMMAND_CALLERS: &[(&str, &str, &str)] = &[
    (
        "webcodex_runner/shell.rs",
        "configured_script_runtime_plan",
        "CONTROL_PLANE_FIXED_PROBE: node --version, constant argv",
    ),
    (
        "main.rs",
        "validation_module_available",
        "CONTROL_PLANE_FIXED_PROBE: python -I -c <constant PROBE>, module from config",
    ),
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
    BypassPattern {
        needle: ".into_command()",
        why: "into_command() is the documented escape hatch out of the broker; a model-reachable \
              routed path must never reach it, or the plan is decorative",
    },
    BypassPattern {
        needle: ".spawn()",
        why: "a routed path must not spawn directly; the broker owns process creation",
    },
];

/// Legacy P1 subset check: listed routed functions must not reach an unbrokered spawn.
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

/// The known-unrouted P1 surfaces, asserted to still exist in the source.
///
/// A surface silently disappearing is as much a lie as one silently appearing:
/// the remaining-surface list in `NORMALIZATION_P1_REPORT.md` is only true if
/// these call sites are still there.
///
/// `validation/execute.rs` is **no longer** in this list. It used to be, and the
/// assertion here used to demand that its `RUNTIME_COMPATIBILITY_TODO` marker
/// stayed put. That assertion is now inverted in P1-I: the file must route. The
/// two tests would have contradicted each other, which is exactly the kind of
/// drift this list exists to prevent — a surface that gets routed must be
/// removed from here in the same change, not asserted in both places.
#[test]
fn known_unrouted_surfaces_are_still_present_and_named() {
    // validation/execute.rs must *not* still be declared unrouted.
    let validation = read_runner_source("validation/execute.rs");
    assert!(
        !validation.contains("RUNTIME_COMPATIBILITY_TODO"),
        "validation/execute.rs is routed now; leaving the un-routed marker would make \
         this list — and the report — wrong"
    );

    // job_manager.rs: the local SSH client, which P1 excludes by scope.
    let jobs = read_runner_source("job_manager.rs");
    assert!(
        jobs.contains("ssh_command_spawn_failed"),
        "the SSH client launch is a known P1 exclusion and must stay visible"
    );

    // detached_job.rs: the durable detached payload. This is the one surface
    // P1b still owes (see DETACHED_DURABLE_NORMALIZATION in the report), so its
    // presence is asserted rather than its absence.
    let detached = read_runner_source("detached_job.rs");
    assert!(
        detached.contains("failed to spawn detached payload"),
        "the detached durable payload is still unrouted and must stay visible; it is \
         recorded as BLOCKED_PROCESS_OWNERSHIP, not as done"
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

/// Every `.rs` file in the runner crate, excluding test-only files.
///
/// `read_runner_source` deliberately resolves only under `src/webcodex_runner`,
/// so `src/main.rs` is invisible to it. This helper walks the crate root instead,
/// which is what makes the `into_command` enumeration below complete: a caller
/// in `main.rs` is exactly the case a narrower walk would miss.
fn runner_crate_source_files() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(relative) = path.strip_prefix(&root) else {
                continue;
            };
            let name = relative.to_string_lossy().replace('\\', "/");
            // Test-only files cannot be production callers, and including them
            // would make the count below meaningless.
            if is_test_only_runner_file(&name) {
                continue;
            }
            if let Ok(source) = std::fs::read_to_string(&path) {
                files.push((name, source));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn is_test_only_runner_file(name: &str) -> bool {
    name.contains("/tests/")
        || name.starts_with("tests/")
        || name.contains("_tests.rs")
        || name.contains("/test_support")
        || name.contains("fake_")
        || name.ends_with("/tests.rs")
}

/// The enclosing `fn` name for a byte offset, scanning backwards for a
/// top-level function signature.
fn enclosing_function(source: &str, offset: usize) -> Option<String> {
    let before = &source[..offset];
    let function_line = before.lines().rev().find(|line| {
        let trimmed = line.trim_start();
        (trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub(crate) fn ")
            || trimmed.starts_with("pub(super) fn ")
            || trimmed.starts_with("pub(in ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub(crate) async fn "))
            && !trimmed.starts_with("//")
    })?;
    let trimmed = function_line.trim_start();
    // Strip the visibility/async/`fn` prefixes *before* splitting on `(`.
    // Splitting first is wrong for `pub(super) fn name(`: the first `(` belongs
    // to the visibility, so the head would be `pub`, and the caller would be
    // attributed to a function called "pub".
    let after_visibility = trimmed
        .trim_start_matches("pub(in ")
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub(super) ")
        .trim_start_matches("pub ")
        .trim_start_matches("async ")
        .trim_start_matches("fn ");
    let after_name = after_visibility
        .split_once('(')
        .map(|(head, _)| head)
        .unwrap_or(after_visibility);
    Some(after_name.trim().to_string())
}
/// P1B: audit every production caller of `into_command()`.
///
/// This is the structural half of closing the escape hatch. The guard above
/// stops a *routed* function from reaching `into_command()`; this test stops
/// the escape hatch from silently growing a new caller anywhere else in the
/// crate. Both halves are needed: the first constrains the model-facing paths,
/// the second makes the allowed set of bypasses explicit and countable.
#[test]
fn p1b_into_command_production_callers_are_enumerated() {
    let mut found: Vec<(String, String)> = Vec::new();
    for (file, source) in runner_crate_source_files() {
        for (index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            if !code.contains(".into_command()") {
                continue;
            }
            let offset = source
                .lines()
                .take(index)
                .map(|line| line.len() + 1)
                .sum::<usize>();
            let function = enclosing_function(&source, offset).unwrap_or_else(|| {
                panic!(
                    "{file}:{}: .into_command() is not inside a recognised fn",
                    index + 1
                )
            });
            found.push((file.clone(), function));
        }
    }

    let mut expected: Vec<(String, String)> = P1B_ALLOWED_INTO_COMMAND_CALLERS
        .iter()
        .map(|(file, function, _)| ((*file).to_string(), (*function).to_string()))
        .collect();
    expected.sort();

    let mut actual = found.clone();
    actual.sort();
    actual.dedup();

    assert_eq!(
        actual, expected,
        "the production callers of into_command() changed. Every caller must be classified in \
         P1B_ALLOWED_INTO_COMMAND_CALLERS with a stated reason; a new caller is either a new \
         model-reachable bypass (unacceptable) or a new fixed control-plane probe (which must be \
         written down here). Found: {found:?}"
    );
}

#[test]
fn p1b_allowed_into_command_callers_are_fixed_control_plane_probes() {
    // Each allowed caller must justify itself in the allowlist. This is a
    // documentation check with teeth: an entry cannot be added without saying
    // *why* it is not model-reachable.
    for (file, function, why) in P1B_ALLOWED_INTO_COMMAND_CALLERS {
        assert!(
            why.contains("CONTROL_PLANE_FIXED_PROBE"),
            "{file}::{function} is allowed to use into_command() but does not declare itself a \
             CONTROL_PLANE_FIXED_PROBE: {why}"
        );
        assert!(
            !why.contains("MODEL_REACHABLE"),
            "{file}::{function} cannot be allowlisted if it is model-reachable: {why}"
        );
    }
}

/// The escape hatch must still exist: it is a legitimate, documented primitive
/// for fixed control-plane probes, and this slice does not remove it.
#[test]
fn p1b_into_command_remains_available_for_control_plane_probes() {
    let source = read_runner_source("local_execution.rs");
    assert!(
        source.contains("pub(crate) fn into_command(self)"),
        "into_command() is the documented control-plane escape hatch and must not be deleted \
         while its fixed-probe users remain"
    );
}

#[test]
fn p1b_routed_paths_forbid_into_command_and_direct_spawn() {
    // The routed functions must not reach any of the three escape primitives.
    // This is the F2 closure: `into_command()` was previously unmonitored, so
    // the invariant rested on convention rather than mechanism.
    let mut offenders = Vec::new();
    for (file, function) in P1_ROUTED_FUNCTIONS {
        let Some(body) = function_body(&read_runner_source(file), function) else {
            continue;
        };
        for needle in [".into_command()", ".spawn()"] {
            if body.contains(needle) {
                offenders.push(format!("{file}::{function}: contains `{needle}`"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "P1-routed execution reached a broker escape primitive:\n{}",
        offenders.join("\n")
    );
}

/// Single-quote a path for `/bin/sh`.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

// ---------------------------------------------------------------------------
// P1B Slice 2A: the catalog git read is brokered, and the remaining
// unconfined project git is *named* rather than spread across the crate.
// ---------------------------------------------------------------------------

/// The unconfined project `git` launcher within `projects/` (legacy subset).
///
/// Slice 2A routed the model-reachable catalog read (`rev-parse` / `log` /
/// `status`, reached by `ToolCall::ListProjects` and every inventory push)
/// through the execution broker. Two callers could not follow it, for a
/// substantive reason recorded in `projects/unconfined_git.rs`: `managed_worktree`
/// runs `git worktree add --detach <destination>` where the destination is
/// *required* to be outside the source root, so the broker's single-root
/// `SandboxPlan::Confined` would deny the write the feature exists to perform.
///
/// This test does not bless that gap. It pins its **extent**: exactly one
/// function may launch a project git directly, it must live in the file named
/// for the purpose, and it must carry a doc comment stating why. A second one
/// appearing anywhere else — including in `catalog.rs` — fails here.
const P1B_UNCONFINED_PROJECT_GIT_LAUNCHERS: &[(&str, &str)] = &[(
    "webcodex_runner/projects/unconfined_git.rs",
    "run_unconfined_git_bounded",
)];

#[test]
fn p1b_unconfined_project_git_has_exactly_one_named_launcher() {
    let mut found: Vec<(String, String)> = Vec::new();
    for (file, source) in runner_crate_source_files() {
        // Only project-side code: the Runner legitimately spawns control-plane
        // processes elsewhere (detached supervisor, browser, computer, plugin,
        // LSP, jobs), and P1B does not claim those.
        if !file.contains("webcodex_runner/projects/") {
            continue;
        }
        // Strip every `#[cfg(test)]` item before scanning. Test fixtures
        // legitimately run `rustc` and `git init` to build a repository, and
        // `enclosing_function` cannot tell those apart from a production
        // launcher — it only knows the nearest `fn`. Scanning the production
        // region is what makes this a claim about shipped code.
        let production = production_region(&source);
        for (index, line) in production.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            if !code.contains("Command::new(") {
                continue;
            }
            let offset = production
                .lines()
                .take(index)
                .map(|line| line.len() + 1)
                .sum::<usize>();
            let function = enclosing_function(&production, offset).unwrap_or_else(|| {
                panic!(
                    "{file}:{}: Command::new( is not inside a recognised fn",
                    index + 1
                )
            });
            found.push((file.clone(), function));
        }
    }

    let mut expected: Vec<(String, String)> = P1B_UNCONFINED_PROJECT_GIT_LAUNCHERS
        .iter()
        .map(|(file, function)| ((*file).to_string(), (*function).to_string()))
        .collect();
    expected.sort();
    let mut actual = found.clone();
    actual.sort();
    actual.dedup();

    assert_eq!(
        actual, expected,
        "the set of direct `Command::new` launchers in projects/ changed. Every one must be \
         classified in P1B_UNCONFINED_PROJECT_GIT_LAUNCHERS. A new one is either a migrated \
         path that should have gone through the broker, or a new authority gap that has to be \
         written down with its reason. Found: {found:?}"
    );
}

/// The catalog read path must not regain a direct launcher, and must name the
/// broker entry point it depends on.
#[test]
fn p1b_catalog_reads_git_only_through_the_broker() {
    let source = read_runner_source("projects/catalog.rs");
    // `#[cfg(test)]` is the production boundary; the test module below it
    // builds a fixture and legitimately names the vocabulary.
    let production = production_region(&source);

    assert!(
        production.contains("git_broker::run_git_bounded_read"),
        "catalog must collect git metadata through the broker's bounded both-streams read"
    );
    for needle in ["Command::new(", "ManagedChild::spawn("] {
        assert!(
            !production.contains(needle),
            "catalog.rs production code must not contain `{needle}`: the model-reachable git \
             read is routed through the execution broker"
        );
    }
    // The display path must never become the execution path again.
    assert!(
        !production.contains("run_git_capture(&resolved_path"),
        "git must be run against the canonical root, never the display path"
    );
    assert!(
        production.contains("match canonical_root.as_deref()"),
        "the git-metadata decision must be driven by the canonical root"
    );
}

/// The doc comment on the unconfined launcher is the reason it is allowed to
/// exist. This is a documentation check with teeth: the entry in
/// `P1B_UNCONFINED_PROJECT_GIT_LAUNCHERS` cannot be added or kept without the
/// file itself explaining the authority problem.
#[test]
fn p1b_unconfined_launcher_documents_why_it_is_not_routed() {
    let source = read_runner_source("projects/unconfined_git.rs");
    for required in [
        "worktree add",
        "writable_roots",
        "ensure_managed_worktree_root",
        "unrouted",
    ] {
        assert!(
            source.contains(required),
            "the unconfined launcher must record why routing is not a mechanical change; \
             expected it to mention `{required}`"
        );
    }
    assert!(
        source.contains("not a resolution") || source.contains("not a migration"),
        "the file must state plainly that this is an open item, not a resolved one"
    );
}
