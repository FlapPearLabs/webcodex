// SPDX-License-Identifier: Apache-2.0
//! Enforcement tests A-E: does a Codex-informed profile actually confine?
//!
//! # What these assert, and what they deliberately do not
//!
//! Each test answers one enforcement question with a **real** confined plan —
//! no permissive profile, no test-only escape. If a plan is refused or fails
//! to compile, that is a failure, not a skip.
//!
//! The one thing these cannot do on the spike host is *run*: this process tree
//! is already inside a sandbox that refuses nested narrowing, so
//! `sandbox_apply` fails before the profile has any effect. A denial observed
//! here would prove nothing — the process never started. So each test first
//! asks the host whether it can apply a restrictive profile at all, and reports
//! `ENV_BLOCKED` when it cannot.
//!
//! `ENV_BLOCKED` is **not** a pass and is never counted as one. The enforcing
//! run is `research/spikes/native-seatbelt-ae.sh`, executed by a human in an
//! ordinary Terminal, and only its output is allowed to move these to PASS.
//!
//! # The distinction that makes B and C meaningful
//!
//! `DENIED_ACTION` and `PROCESS_COULD_NOT_START` are different outcomes. A
//! `SIGABRT` from the dynamic loader, or rc=71 from `sandbox-exec` itself, means
//! the program never ran — reporting that as "the read was correctly denied"
//! would be a false pass. Test B and test C therefore assert the *positive*
//! first: the process starts and does something observable. Only then is the
//! denial accepted as enforcement.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use webcodex_process::execution_broker::{
    BrokerError, ExecutionBroker, NetworkPolicy, SandboxPlan, SpawnSpec, StreamPolicy,
    TrustedToolchainRoot,
};

// ---------------------------------------------------------------------------
// Self-generated fixtures
//
// No test may depend on a fixed path left behind by an earlier run. Each test
// builds its own TEMP_ROOT with a workspace/ and an outside/ sibling, so a
// stale /tmp/webcodex-sandbox-spike can never make a test pass or fail for the
// wrong reason.
// ---------------------------------------------------------------------------

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    workspace: PathBuf,
    outside: PathBuf,
    inside_file: PathBuf,
    outside_file: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&outside).expect("create outside");

        let inside_file = workspace.join("inside.txt");
        let outside_file = outside.join("outside.txt");
        std::fs::write(&inside_file, "WORKSPACE_INSIDE\n").expect("write inside");
        std::fs::write(&outside_file, "OUTSIDE_SECRET\n").expect("write outside");

        Self {
            _dir: dir,
            root,
            workspace,
            outside,
            inside_file,
            outside_file,
        }
    }
}

/// Absolute path, canonicalized, as a string for embedding in a shell script.
fn sh_quote_path(p: &Path) -> String {
    p.canonicalize()
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .into_owned()
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

fn run(spec: &SpawnSpec) -> (Vec<u8>, Vec<u8>, i32) {
    let broker = ExecutionBroker::new();
    match broker.spawn(spec) {
        Err(e) => panic!("broker refused to spawn: {e}"),
        Ok(mut child) => {
            let out = read_stdout(&mut child);
            let err = read_stderr(&mut child);
            let status = child.wait().expect("wait");
            (out, err, status.code().unwrap_or(-1))
        }
    }
}

/// Whether this host will apply a restrictive profile at all.
///
/// One probe only, and it is the one verified to work on a real Terminal:
/// `(version 1)(allow default)(deny network*)`.
///
/// A second probe on `(allow default)(deny file-read*)` used to be required
/// here. That was wrong. A blanket `deny file-read*` cuts off the system reads
/// `/usr/bin/true` needs in order to start, so it fails with rc=134 even on a
/// host that applies restrictive profiles perfectly well — which made the
/// precondition report ENV_BLOCKED on capable hosts.
///
/// The precondition that actually matters is narrow: `sandbox-exec` exists, and
/// a profile that narrows something *without preventing the target from
/// starting* can be applied. Only a failure here means `sandbox_apply` itself
/// failed.
fn host_allows_restrictive_profiles() -> bool {
    std::process::Command::new("/usr/bin/sandbox-exec")
        .arg("-p")
        .arg("(version 1)(allow default)(deny network*)")
        .arg("/usr/bin/true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

macro_rules! require_enforcement_capable_host {
    ($label:literal) => {
        if !host_allows_restrictive_profiles() {
            eprintln!(
                "ENV_BLOCKED[{}]: this host refuses restrictive Seatbelt profiles at \
                 sandbox_apply, so enforcement is unmeasurable here. NOT a pass. \
                 Run research/spikes/native-seatbelt-ae.sh from an ordinary Terminal.",
                $label
            );
            return;
        }
    };
}

/// Plan A: the workspace is readable and writable, nothing else, no network.
fn plan_workspace_only(ws: &Path) -> SandboxPlan {
    SandboxPlan::read_write_in(&[ws.to_path_buf()])
}

// ---------------------------------------------------------------------------
// The cwd invariant
//
// These are pure refusals: they never launch anything, so they run for real
// on any host, nested sandbox or not. They live in the integration test
// because the invariant is a property of the **public** API — a downstream
// caller must not be able to obtain a confined action rooted outside its own
// authority by naming a different `cwd`.
// ---------------------------------------------------------------------------

/// A cwd inside a writable root is accepted.
///
/// The positive case matters as much as the refusals: an invariant that rejects
/// everything would also refuse everything, and would look correct for the
/// wrong reason.
#[test]
fn cwd_inside_a_writable_root_is_accepted() {
    let fx = Fixture::new();
    let nested = fx.workspace.join("nested");
    std::fs::create_dir_all(&nested).expect("create nested");

    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new("/bin/true", &nested, plan_workspace_only(&fx.workspace));
    assert!(
        broker.check_cwd(&spec).is_ok(),
        "a cwd nested inside a writable root must be accepted, got {:?}",
        broker.check_cwd(&spec).err()
    );
}

/// A cwd that is *itself* a readable-only root is accepted.
///
/// A read-only root must be usable as a working directory: refusing it would
/// force every read-only action to also be granted write access to its own
/// working directory, which is authority nobody asked for.
#[test]
fn cwd_inside_a_readable_only_root_is_accepted() {
    let fx = Fixture::new();
    let nested = fx.outside.join("nested");
    std::fs::create_dir_all(&nested).expect("create nested");

    let broker = ExecutionBroker::new();
    let plan = SandboxPlan::Confined {
        writable_roots: vec![fx.workspace.clone()],
        readable_roots: vec![fx.outside.clone()],
        network: NetworkPolicy::Deny,
    };
    let spec = SpawnSpec::new("/bin/true", &nested, plan);
    assert!(
        broker.check_cwd(&spec).is_ok(),
        "a readable-only root must be a legal working directory, got {:?}",
        broker.check_cwd(&spec).err()
    );
}

/// A cwd that is a **sibling** of every root is refused.
///
/// This is the case the native probe actually hit: `cwd = TEMP_ROOT` while the
/// plan granted only `TEMP_ROOT/workspace`. `/tmp` is a symlink to
/// `/private/tmp` on macOS, so a literal prefix check against the workspace
/// would not catch the parent — only canonicalization does.
#[test]
fn cwd_in_the_parent_of_every_root_is_refused() {
    let fx = Fixture::new();
    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new("/bin/true", &fx.root, plan_workspace_only(&fx.workspace));
    let err = broker
        .check_cwd(&spec)
        .expect_err("parent of the root must be refused");
    assert!(
        matches!(err, BrokerError::CwdOutsideSandboxRoots { .. }),
        "got {err:?}"
    );
    let text = err.to_string();
    assert!(
        text.contains("not inside any of"),
        "the refusal must name the roots it tried, got: {text}"
    );
    assert!(
        text.contains("workspace"),
        "the refusal must name the writable root, got: {text}"
    );
}

/// A cwd that is a symlink escaping every root is refused.
///
/// Without canonicalization this is the bypass: the literal path
/// `workspace/escape` is *inside* the root, so a string comparison would accept
/// it, while the directory the child actually lands in is `outside/`. This is
/// the case a "just check the string" implementation gets wrong.
#[test]
fn cwd_behind_a_symlink_that_escapes_every_root_is_refused() {
    let fx = Fixture::new();
    let link = fx.workspace.join("escape");
    std::os::unix::fs::symlink(&fx.outside, &link).expect("symlink");

    let broker = ExecutionBroker::new();
    // The literal path is inside the writable root; its target is not.
    assert!(
        link.starts_with(&fx.workspace),
        "precondition: the symlink is lexically inside the root"
    );
    let spec = SpawnSpec::new("/bin/true", &link, plan_workspace_only(&fx.workspace));
    let err = broker
        .check_cwd(&spec)
        .expect_err("a symlink escape must be refused");
    assert!(
        matches!(err, BrokerError::CwdOutsideSandboxRoots { .. }),
        "got {err:?}"
    );
    assert!(
        err.to_string().contains("not inside any of"),
        "the refusal must report the resolved target, got: {err}"
    );
}

/// A cwd that does not exist is refused, with a reason that says so.
///
/// Canonicalization failing is a refusal, not a silent fall back to the
/// broker's own directory: "run it somewhere else" is not a decision this
/// layer is allowed to make.
#[test]
fn cwd_that_does_not_exist_is_refused() {
    let fx = Fixture::new();
    let missing = fx.workspace.join("no-such-directory");
    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new("/bin/true", &missing, plan_workspace_only(&fx.workspace));
    let err = broker
        .check_cwd(&spec)
        .expect_err("a missing cwd must be refused");
    assert!(
        matches!(err, BrokerError::CwdOutsideSandboxRoots { .. }),
        "got {err:?}"
    );
    assert!(
        err.to_string().contains("does not resolve"),
        "the refusal must distinguish 'missing' from 'outside', got: {err}"
    );
}

/// A cwd that is a **file** is refused.
///
/// `chdir` into a file fails, but only inside the child — after a profile has
/// been applied and a process exists. Catching it here keeps the failure where
/// it can still be reported as a refusal.
#[test]
fn cwd_that_is_a_file_is_refused() {
    let fx = Fixture::new();
    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new(
        "/bin/true",
        &fx.inside_file,
        plan_workspace_only(&fx.workspace),
    );
    let err = broker
        .check_cwd(&spec)
        .expect_err("a file must not be a working directory");
    assert!(
        matches!(err, BrokerError::CwdOutsideSandboxRoots { .. }),
        "got {err:?}"
    );
    assert!(err.to_string().contains("not a directory"), "got {err}");
}

/// The invariant is enforced on the **spawn** path, not only on the checker.
///
/// Calling `check_cwd` directly proves the rule exists. This proves nothing
/// can route around it: `spawn` refuses before a process exists.
#[test]
fn spawn_refuses_a_cwd_outside_the_plan_before_any_process_exists() {
    let fx = Fixture::new();
    let broker = ExecutionBroker::new();
    let plan = plan_workspace_only(&fx.workspace);

    // cwd = the parent of every root. This is the exact shape the native probe
    // used to launch with, and the reason every confined child printed
    // `getcwd: cannot access parent directories`.
    let spec = SpawnSpec::new("/bin/sh", &fx.root, plan.clone())
        .arg("-c")
        .arg("pwd")
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    let err = broker.spawn(&spec).expect_err("spawn must refuse");
    assert!(
        matches!(err, BrokerError::CwdOutsideSandboxRoots { .. }),
        "got {err:?}"
    );

    // And a cwd inside the plan still spawns for real, so the test above is a
    // gate and not a blanket refusal. Run without a sandbox-capability gate:
    // if this host refuses restrictive profiles, the *spawn* fails — which is
    // still not a refusal, and must not be read as one.
    let ok_spec = SpawnSpec::new("/bin/sh", &fx.workspace, plan)
        .arg("-c")
        .arg("pwd")
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    match broker.spawn(&ok_spec) {
        Ok(mut child) => {
            let _ = read_stdout(&mut child);
            let _ = read_stderr(&mut child);
            let _ = child.wait();
        }
        Err(BrokerError::CwdOutsideSandboxRoots { .. }) => {
            panic!("a cwd inside the plan must not be refused by the cwd invariant")
        }
        Err(other) => eprintln!(
            "note: host could not apply the profile ({}); the positive case is \
             covered by the refusal assertions above",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// A — workspace read/write
// ---------------------------------------------------------------------------

#[test]
fn test_a_workspace_read_write() {
    require_enforcement_capable_host!("A");

    let fx = Fixture::new();
    let ws = sh_quote_path(&fx.workspace);
    let inside = sh_quote_path(&fx.inside_file);
    let new_file = sh_quote_path(&fx.workspace.join("new.txt"));

    // One process performs both actions, so a single rc distinguishes
    // "workspace usable" from "workspace unusable".
    let script = format!(
        r#"cat "{inside}" > /dev/null && printf 'CREATED' > "{new_file}" && cat "{new_file}""#
    );
    let spec = SpawnSpec::new("/bin/sh", &fx.workspace, plan_workspace_only(&fx.workspace))
        .arg("-c")
        .arg(&script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);

    let (out, err, code) = run(&spec);
    assert_eq!(
        code,
        0,
        "A: workspace read+write must succeed; stderr={}",
        String::from_utf8_lossy(&err)
    );
    assert_eq!(String::from_utf8_lossy(&out).trim(), "CREATED");
    assert!(
        fx.workspace.join("new.txt").exists(),
        "the write must be visible outside the sandbox"
    );
    let _ = ws;
}

// ---------------------------------------------------------------------------
// B — external read is denied, and the process demonstrably started
// ---------------------------------------------------------------------------

#[test]
fn test_b_external_read_is_denied() {
    require_enforcement_capable_host!("B");

    let fx = Fixture::new();
    let outside = sh_quote_path(&fx.outside_file);
    let inside = sh_quote_path(&fx.inside_file);

    // The child proves it is running by reading something it *is* allowed to
    // read, then attempts the forbidden read — and *catches the denial itself*,
    // exiting 0.
    //
    // This is the corrected oracle. The previous version appended a bare
    // `cat "{outside}"` and required the process to exit 0, which is backwards:
    // `cat` returns non-zero when it is denied, so correct enforcement produced
    // a non-zero exit and the test FAILED. Letting the child report the denial
    // separates "the sandbox denied it" from "the program could not start",
    // which a non-zero exit cannot do.
    let script = format!(
        r#"cat "{inside}" > /dev/null && printf 'B_STARTED_OK' && if cat "{outside}" 2>/dev/null; then printf 'B_LEAK'; exit 1; else printf 'B_DENIED'; exit 0; fi"#
    );
    let spec = SpawnSpec::new("/bin/sh", &fx.workspace, plan_workspace_only(&fx.workspace))
        .arg("-c")
        .arg(&script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);

    let (out, err, code) = run(&spec);

    assert_eq!(
        code,
        0,
        "B: the child must report the denial and exit 0; a non-zero exit means \
         something other than enforcement went wrong. stderr={}",
        String::from_utf8_lossy(&err)
    );
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("B_STARTED_OK"),
        "B: SANDBOX_APPLIED_AND_ACTION_DENIED requires proof the process started; \
         stdout was {:?}, stderr was {:?}",
        text,
        String::from_utf8_lossy(&err)
    );
    assert!(
        text.contains("B_DENIED"),
        "B: the child must observe and report the denial; stdout was {text:?}"
    );
    assert!(
        !text.contains("B_LEAK"),
        "B: the denied read must not return data: {text}"
    );
}

// ---------------------------------------------------------------------------
// C — descendants inherit the profile
//
// **System-only, and deliberately so.**
//
// This test previously ran `sh -> python3 -> open(outside.txt)`. It failed on
// hosts where `/usr/bin/python3` is only an `xcode-select` stub, and it failed
// on hosts where a working `python3` lives in a toolchain prefix outside
// `EnvPolicy::Minimal`'s `PATH=/usr/bin:/bin`.
//
// Both of those failures say something about *interpreter availability*, not
// about whether a grandchild inherits its parent's Seatbelt profile. Conflating
// them meant the security property could only ever be reported as "failed" for
// reasons that had nothing to do with confinement — which is how a real
// enforcement bug would have been lost in the noise.
//
// The security claim ("a descendant two levels down is still confined") needs no
// interpreter at all: `/bin/sh -> /bin/sh -> /bin/cat` exercises exactly the
// inheritance edge under test, using only binaries the profile already permits.
// Whether a host's python3 or node happens to be runnable is a separate,
// separately-reported fact — see the `RUNTIME_*` probes below.
// ---------------------------------------------------------------------------

#[test]
fn test_c_descendant_inherits_the_profile_two_levels_deep() {
    require_enforcement_capable_host!("C");

    let fx = Fixture::new();
    let inside = sh_quote_path(&fx.inside_file);
    let outside = sh_quote_path(&fx.outside_file);

    // Two levels of descendant, all system binaries:
    //   outer /bin/sh  ->  inner /bin/sh  ->  /bin/cat outside.txt
    //
    // The outer shell proves it started. The inner shell proves the profile is
    // inherited by a *grandchild* — one level of inheritance would not rule out
    // the profile being re-applied only to direct children. `cat` performs the
    // denied read and reports the denial itself, exiting 0, for the reason
    // spelled out in test B.
    let script = format!(
        r#"cat "{inside}" > /dev/null && printf 'C_OUTER_STARTED' && /bin/sh -c '
if /bin/cat "{outside}" 2>/dev/null; then
  printf "C_LEAK"
  exit 1
else
  printf "C_INNER_STARTED C_DENIED"
  exit 0
fi'"#
    );
    let spec = SpawnSpec::new("/bin/sh", &fx.workspace, plan_workspace_only(&fx.workspace))
        .arg("-c")
        .arg(&script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);

    let (out, err, code) = run(&spec);
    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        code,
        0,
        "C: the shells must run and report; stderr={}",
        String::from_utf8_lossy(&err)
    );
    assert!(
        text.contains("C_OUTER_STARTED"),
        "C: the outer shell must have started; stdout={text:?}"
    );
    assert!(
        text.contains("C_INNER_STARTED"),
        "C: the inner shell (a grandchild) must have started; stdout={text:?}"
    );
    assert!(
        text.contains("C_DENIED"),
        "C: the grandchild must observe and report the denial; stdout={text:?}"
    );
    assert!(
        !text.contains("C_LEAK"),
        "C: the grandchild must not read the denied file; stdout={text:?}"
    );
}

/// **Runtime compatibility, not enforcement.**
///
/// Whether a non-system interpreter can start under this profile is a real
/// question about the design — the toolchain grant exists precisely to answer
/// it — but it is a question about the *host's* interpreter layout, not about
/// whether the profile confines. It is reported separately and never folded into
/// the security verdict, because a host with no usable `node` would otherwise
/// make an unrelated enforcement regression invisible.
#[test]
fn runtime_node_can_start_under_a_trusted_toolchain_grant() {
    let fx = Fixture::new();

    let node = match which("node") {
        Some(n) => n,
        None => {
            eprintln!("RUNTIME_NODE=UNAVAILABLE (node not present on this host)");
            return;
        }
    };
    let toolchain = match webcodex_process::execution_broker::TrustedToolchainRoot::resolve(&node) {
        Ok(root) => vec![root],
        Err(e) => {
            eprintln!(
                "RUNTIME_NODE=UNAVAILABLE ({} is not in a recognised toolchain \
                 layout: {e})",
                node.display()
            );
            return;
        }
    };

    let broker = ExecutionBroker::new();
    let spec = SpawnSpec::new("/bin/sh", &fx.workspace, plan_workspace_only(&fx.workspace))
        .arg("-c")
        .arg("node -e 'process.stdout.write(\"NODE_OK\")'")
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);

    let outcome = match broker.spawn_with_toolchain(&spec, &toolchain) {
        Ok(mut child) => {
            let out = read_stdout(&mut child);
            let err = read_stderr(&mut child);
            let status = child.wait().expect("wait");
            (
                String::from_utf8_lossy(&out).into_owned(),
                String::from_utf8_lossy(&err).into_owned(),
                status.code().unwrap_or(-1),
            )
        }
        // A host that cannot apply the profile at all: unmeasurable, not failed.
        Err(BrokerError::CwdOutsideSandboxRoots { .. }) => {
            panic!("node probe used a cwd outside the plan")
        }
        Err(e) => {
            eprintln!("RUNTIME_NODE=UNAVAILABLE (broker refused: {e})");
            return;
        }
    };

    let (out, err, code) = outcome;
    if code == 0 && out.contains("NODE_OK") {
        eprintln!("RUNTIME_NODE=PASS");
    } else {
        eprintln!(
            "RUNTIME_NODE=FAIL (rc={code} stdout={out:?} stderr={err:?}); \
             this is an interpreter-availability result, NOT an enforcement result"
        );
    }
}

// ---------------------------------------------------------------------------
// D — same action, different plan, different outcome
// ---------------------------------------------------------------------------

#[test]
fn test_d_same_action_differs_between_profiles() {
    require_enforcement_capable_host!("D");

    let fx = Fixture::new();
    let outside = sh_quote_path(&fx.outside_file);

    let attempt = |plan: SandboxPlan| {
        let spec = SpawnSpec::new("/bin/cat", &fx.workspace, plan)
            .arg(&outside)
            .stdout(StreamPolicy::Piped)
            .stderr(StreamPolicy::Piped);
        run(&spec)
    };

    // Plan A: workspace only -> denied.
    let (out_a, _err_a, code_a) = attempt(plan_workspace_only(&fx.workspace));
    assert_ne!(code_a, 0, "D: plan A must deny the outside read");
    assert!(
        !String::from_utf8_lossy(&out_a).contains("OUTSIDE_SECRET"),
        "D: plan A returned the secret"
    );

    // Plan B: same action, but the outside fixture is explicitly readable.
    let (out_b, err_b, code_b) = attempt(SandboxPlan::Confined {
        writable_roots: vec![fx.workspace.clone()],
        readable_roots: vec![fx.outside.clone()],
        network: NetworkPolicy::Deny,
    });
    assert_eq!(
        code_b,
        0,
        "D: plan B must allow the same read; stderr={}",
        String::from_utf8_lossy(&err_b)
    );
    assert!(
        String::from_utf8_lossy(&out_b).contains("OUTSIDE_SECRET"),
        "D: plan B must actually return the data"
    );
}

// ---------------------------------------------------------------------------
// E — network is denied, with an out-of-sandbox positive control
//
// **System-only.** `nc` replaces a python socket here for the same reason C
// stopped using python: a missing or stubbed interpreter must not be able to
// fail a network-enforcement test.
//
// The positive control is what makes the negative result meaningful. Without
// it, "the connect failed" has two indistinguishable causes — the profile
// denied it, or nothing was listening. The control connects to the *same*
// listener from *outside* the profile first; if that succeeds, a later failure
// under the profile is attributable to the profile.
// ---------------------------------------------------------------------------

/// Path to the system `nc`, which is the only network client this test uses.
const SYSTEM_NC: &str = "/usr/bin/nc";

#[test]
fn test_e_network_denied() {
    require_enforcement_capable_host!("E");

    if !Path::new(SYSTEM_NC).is_file() {
        eprintln!("BLOCKED[E]: {SYSTEM_NC} not present on this host");
        return;
    }

    // Bound by this process, OUTSIDE the sandbox. `local_addr` succeeding is
    // the proof that a denial is attributable to the profile rather than to a
    // missing server.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();

    // ---- positive control: unsandboxed nc, same listener -----------------
    let control = std::process::Command::new(SYSTEM_NC)
        .args(["-w", "2", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run unsandboxed nc");
    assert!(
        control.success(),
        "E: PRECONDITION FAILED — unsandboxed {SYSTEM_NC} could not reach the \
         listener on 127.0.0.1:{port}, so a sandboxed failure would prove nothing"
    );

    // The listener accepted the control connection; drain it so the socket is
    // in a clean state for the sandboxed attempt.
    drop(listener.accept());

    let fx = Fixture::new();
    let script = format!(
        r#"printf 'E_STARTED_OK'; if {SYSTEM_NC} -w 2 127.0.0.1 {port} < /dev/null; then printf 'E_CONNECTED'; else printf 'E_DENIED'; fi; exit 0"#
    );
    let spec = SpawnSpec::new("/bin/sh", &fx.workspace, plan_workspace_only(&fx.workspace))
        .arg("-c")
        .arg(&script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);

    let (out, err, code) = run(&spec);
    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        code,
        0,
        "E: the child must start and report; stderr={}",
        String::from_utf8_lossy(&err)
    );
    assert!(
        text.contains("E_STARTED_OK"),
        "E: the child must have started; stdout={text:?}"
    );
    assert!(
        !text.contains("E_CONNECTED"),
        "E: the connection must be denied; stdout={text:?}"
    );
    assert!(
        text.contains("E_DENIED"),
        "E: the denial must be observed by the child, not a silent hang; stdout={text:?}"
    );
}

/// Locate an executable on the host's PATH, for SKIP_REASON reporting.
fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

// ---------------------------------------------------------------------------
// Toolchain grant authority
//
// This lives in an integration test on purpose. An integration test links the
// library the way a *downstream crate* would, so it can only reach the public
// API. If a caller could hand the broker an arbitrary directory as a toolchain
// root, the test below would compile. It does not compile, because
// `spawn_with_toolchain` takes `&[TrustedToolchainRoot]` and that type has no
// public constructor.
// ---------------------------------------------------------------------------

/// **A production caller cannot obtain a whole-filesystem read grant.**
///
/// `/` is the sharpest case: it is absolute, it exists, and it is not inside
/// `$HOME`, so every check the previous `&[PathBuf]` API performed would have
/// passed it. The resulting profile would contain `(subpath "/")`.
#[test]
fn arbitrary_filesystem_root_cannot_be_requested_as_a_toolchain_grant() {
    // The resolver is the only way to mint a grant, and it demands an
    // executable inside a recognised prefix.
    for bogus in ["/", "/usr", "/opt", "/System", "/private", "/Users"] {
        assert!(
            TrustedToolchainRoot::resolve(Path::new(bogus)).is_err(),
            "{bogus} must be rejected as a toolchain root"
        );
    }

    // A directory is not an interpreter, so pointing at one is rejected even
    // when it sits inside a recognised prefix.
    for dir in ["/opt", "/usr", "/usr/bin"] {
        if Path::new(dir).is_dir() {
            assert!(
                TrustedToolchainRoot::resolve(Path::new(dir)).is_err(),
                "{dir} is a directory and must not be accepted as an executable"
            );
        }
    }

    // And a file that is not in any recognised layout — here, a temporary file
    // under the system temp dir, which on macOS is /var/folders/... — cannot be
    // turned into a grant either.
    let stray = tempfile::NamedTempFile::new().expect("temp file");
    assert!(
        TrustedToolchainRoot::resolve(stray.path()).is_err(),
        "a file outside every recognised toolchain prefix must be rejected: {}",
        stray.path().display()
    );
}

/// A real executable in a recognised prefix *is* accepted, and yields a bounded
/// prefix — so the restriction above is a real gate, not a blanket refusal.
#[test]
fn real_toolchain_executable_is_accepted_and_yields_a_bounded_prefix() {
    let true_bin = Path::new("/usr/bin/true");
    if !true_bin.is_file() {
        eprintln!("SKIP_REASON: /usr/bin/true not present on this host");
        return;
    }
    let root = TrustedToolchainRoot::resolve(true_bin).expect("system binary is trusted");
    let granted = root.as_path().to_string_lossy().into_owned();
    assert_ne!(granted, "/", "the resolver must never yield /");
    assert!(
        Path::new(&granted).is_dir(),
        "the granted prefix must be a real directory: {granted}"
    );

    // The grant is a *prefix of the executable's location*, not the whole disk.
    let exe = true_bin.canonicalize().expect("canonical");
    assert!(
        exe.starts_with(&granted),
        "{exe:?} should live under the granted prefix {granted}"
    );
}
