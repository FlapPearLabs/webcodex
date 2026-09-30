// SPDX-License-Identifier: Apache-2.0
//! Native enforcement probe: A-E plus runtime compatibility, for a
//! human-run Terminal.
//!
//! # Why this binary exists
//!
//! The automated tests cannot measure enforcement when the process tree is
//! already inside a sandbox that refuses nested narrowing: `sandbox_apply`
//! fails before the profile has any effect, so a "denial" would be
//! indistinguishable from "the program never started". This binary is meant to
//! be run from an **ordinary Terminal.app**, outside any agent sandbox, by
//! `research/spikes/native-seatbelt-ae.sh`.
//!
//! It uses the production [`ExecutionBroker`] and the production profile
//! compiler — including the production **cwd invariant**: every confined action
//! is rooted in `fixture.workspace`, the only directory the plan grants. There
//! is no permissive path and no test-only shortcut here. If this binary reports
//! a pass, the profile that produced it is the profile WebCodex would ship.
//!
//! # Security gate vs. runtime compatibility
//!
//! The gate (`NATIVE_A..E`, `NATIVE_SECURITY_ALL_PASS`) answers exactly one
//! question: **does the Codex-informed profile confine a system-binary action?**
//! It uses only `/bin/sh`, `/bin/cat`, and `/usr/bin/nc` — interpreters the
//! profile already permits, with no toolchain grant and no `PATH` dependency.
//!
//! The runtime lines (`RUNTIME_PYTHON`, `RUNTIME_NODE`) answer a *different*
//! question — whether a non-system interpreter happens to be runnable on this
//! host's layout — and are reported **separately**. They never enter the
//! security verdict. A host whose `python3` is only an `xcode-select` stub must
//! not be able to fail a filesystem-enforcement test, which is how a real
//! confinement regression would have been hidden in the noise.
//!
//! # Output contract
//!
//! A machine-readable block at the end, one `KEY=value` per line, so the
//! wrapper script never has to parse prose:
//!
//! ```text
//! NATIVE_A=true
//! NATIVE_B=true
//! NATIVE_C=true
//! NATIVE_D=true
//! NATIVE_E=true
//! NATIVE_SECURITY_ALL_PASS=true
//! RUNTIME_PYTHON=PASS
//! RUNTIME_NODE=FAIL
//! ```

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use webcodex_process::execution_broker::{
    ExecutionBroker, NetworkPolicy, SandboxPlan, SpawnSpec, StreamPolicy, TrustedToolchainRoot,
};

/// The only network client this probe uses for the security gate.
const SYSTEM_NC: &str = "/usr/bin/nc";

// ---------------------------------------------------------------------------
// Fixture: generated per run, never a fixed /tmp path
// ---------------------------------------------------------------------------

struct Fixture {
    _dir: tempfile::TempDir,
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
            workspace,
            outside,
            inside_file,
            outside_file,
        }
    }
}

/// Canonicalized absolute path, safe to embed in a single-quoted shell string.
fn q(p: &Path) -> String {
    p.canonicalize()
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .replace('\'', "'\\''")
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(program))
        .find(|c| c.is_file())
}

/// Read-only prefixes needed to *start* an interpreter on this machine.
///
/// The Codex-derived platform defaults stop at fixed system prefixes, so a
/// Homebrew-installed `node` is not executable under them, and a host
/// `python3` that lives outside `/usr/bin` is not either. Each root here is
/// minted by `TrustedToolchainRoot::resolve`, which derives a **bounded**
/// prefix from a real executable in a recognised toolchain layout. That is
/// deliberate: this function looks up the interpreter on `PATH` and vouches for
/// the prefix it lives in. It never accepts a caller-supplied directory, so
/// there is no way to ask for `/` here — or anywhere else.
fn toolchain_roots_for(program: &str) -> Vec<TrustedToolchainRoot> {
    let Some(path) = which(program) else {
        return Vec::new();
    };
    // resolve() canonicalizes, requires a regular file, and refuses any prefix
    // that is not a recognised toolchain layout. A failure is not fatal: the
    // runtime check that needs it will report why it could not start.
    match TrustedToolchainRoot::resolve(&path) {
        Ok(root) => vec![root],
        Err(e) => {
            eprintln!("note: no trusted toolchain root for {program} at {path:?}: {e}");
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Execution helper
// ---------------------------------------------------------------------------

struct Outcome {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run(spec: &SpawnSpec, toolchain: &[TrustedToolchainRoot]) -> Result<Outcome, String> {
    let broker = ExecutionBroker::new();
    let mut child = broker
        .spawn_with_toolchain(spec, toolchain)
        .map_err(|e| format!("broker refused to spawn: {e}"))?;

    let mut out = Vec::new();
    let mut err = Vec::new();
    if let Some(mut s) = child.child_mut().stdout.take() {
        let _ = s.read_to_end(&mut out);
    }
    if let Some(mut s) = child.child_mut().stderr.take() {
        let _ = s.read_to_end(&mut err);
    }
    let status = child.wait().map_err(|e| format!("wait failed: {e}"))?;
    Ok(Outcome {
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
        code: status.code().unwrap_or(-1),
    })
}

/// Confined action rooted in `fixture.workspace` — the only directory the plan
/// grants. This is the fix for the previous run's `getcwd: cannot access parent
/// directories` noise: the cwd is never the fixture's parent.
fn sh_in(
    workspace: &Path,
    plan: SandboxPlan,
    script: &str,
    toolchain: &[TrustedToolchainRoot],
) -> Result<Outcome, String> {
    let spec = SpawnSpec::new("/bin/sh", workspace, plan)
        .arg("-c")
        .arg(script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    run(&spec, toolchain)
}

fn report(label: &str, ok: bool, detail: &str) {
    println!("{label} {}", if ok { "PASS" } else { "FAIL" });
    if !detail.is_empty() {
        for line in detail.lines() {
            println!("    | {line}");
        }
    }
}

fn plan_for(workspace: &Path) -> SandboxPlan {
    SandboxPlan::read_write_in(&[workspace.to_path_buf()])
}

fn main() {
    // Fail loudly rather than quietly reporting ENV_BLOCKED as success.
    if !host_allows_restrictive_profiles() {
        eprintln!(
            "FATAL: this process cannot apply a restrictive Seatbelt profile.\n\
             Run this from an ordinary Terminal.app, not from inside an agent sandbox."
        );
        std::process::exit(2);
    }

    let fx = Fixture::new();

    // ---- Precondition: a confined process can start at all ---------------
    // This folds the old T0/T1 into a single gate on the production path. It is
    // run with `cwd = fixture.workspace` like every other confined action, so
    // it exercises the cwd invariant too.
    {
        let o = sh_in(
            &fx.workspace,
            plan_for(&fx.workspace),
            "printf 'PRECOND_OK'",
            &[],
        );
        match &o {
            Ok(r) if r.code == 0 && r.stdout.contains("PRECOND_OK") => {
                report("precondition /bin/sh starts under the profile", true, "");
            }
            other => {
                let detail = match other {
                    Ok(r) => format!("rc={} stdout={:?} stderr={}", r.code, r.stdout, r.stderr),
                    Err(e) => e.clone(),
                };
                report(
                    "precondition /bin/sh starts under the profile",
                    false,
                    &detail,
                );
                eprintln!("FATAL: a confined process cannot start; the A-E gate is meaningless.");
                std::process::exit(2);
            }
        }
    }

    let mut security: Vec<(&str, bool)> = Vec::new();

    // ---- A: workspace read + write --------------------------------------
    {
        let inside = q(&fx.inside_file);
        let created = q(&fx.workspace.join("new.txt"));
        let script = format!(
            r#"cat "{inside}" > /dev/null && printf 'CREATED' > "{created}" && cat "{created}""#
        );
        let o = sh_in(&fx.workspace, plan_for(&fx.workspace), &script, &[]);
        let wrote = fx.workspace.join("new.txt").exists();
        let ok = matches!(&o, Ok(r) if r.code == 0 && r.stdout.trim() == "CREATED") && wrote;
        let detail = match &o {
            Ok(r) => format!(
                "rc={} stdout={:?} file_on_disk={} stderr={}",
                r.code,
                r.stdout.trim(),
                wrote,
                r.stderr.trim()
            ),
            Err(e) => e.clone(),
        };
        report("A workspace read+write", ok, &detail);
        security.push(("NATIVE_A", ok));
    }

    // ---- B: external read denied, process demonstrably started ----------
    {
        let inside = q(&fx.inside_file);
        let outside = q(&fx.outside_file);
        // The denial is caught by the *test program*, which then exits 0.
        //
        // An earlier version asserted on a non-zero exit, on the theory that a
        // denied read must make the process fail. That is backwards: `cat`
        // returns non-zero when it is denied, so correct enforcement produced a
        // non-zero exit and the test FAILED. Having the child report the denial
        // and exit cleanly is what separates "the sandbox denied it" from "the
        // program could not start" — a non-zero exit cannot.
        let script = format!(
            r#"cat "{inside}" > /dev/null && printf 'B_STARTED_OK' && if cat "{outside}" 2>/dev/null; then printf 'B_LEAK'; exit 1; else printf 'B_DENIED'; exit 0; fi"#
        );
        let o = sh_in(&fx.workspace, plan_for(&fx.workspace), &script, &[]);
        let ok = match &o {
            Ok(r) => {
                r.code == 0
                    && r.stdout.contains("B_STARTED_OK")
                    && r.stdout.contains("B_DENIED")
                    && !r.stdout.contains("B_LEAK")
            }
            Err(_) => false,
        };
        let detail = match &o {
            Ok(r) => format!(
                "rc={} stdout={:?} stderr={}",
                r.code,
                r.stdout.trim(),
                r.stderr.trim()
            ),
            Err(e) => e.clone(),
        };
        report("B external read denied", ok, &detail);
        security.push(("NATIVE_B", ok));
    }

    // ---- C: descendants inherit, two levels deep, system-only -----------
    {
        let inside = q(&fx.inside_file);
        let outside = q(&fx.outside_file);
        // Two levels of descendant, all system binaries:
        //   outer /bin/sh  ->  inner /bin/sh  ->  /bin/cat outside.txt
        //
        // The outer shell proves it started; the inner shell proves the profile
        // is inherited by a *grandchild* — a single level of inheritance would
        // not rule out the profile being re-applied only to direct children.
        // `cat` does the denied read and reports the denial itself, exiting 0.
        //
        // This replaced the old `sh -> python3 -> open()` form on purpose: a
        // missing or stubbed python3 is an interpreter-availability fact, not a
        // confinement fact, and must not gate the security verdict.
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
        let o = sh_in(&fx.workspace, plan_for(&fx.workspace), &script, &[]);
        let ok = match &o {
            Ok(r) => {
                r.code == 0
                    && r.stdout.contains("C_OUTER_STARTED")
                    && r.stdout.contains("C_INNER_STARTED")
                    && r.stdout.contains("C_DENIED")
                    && !r.stdout.contains("C_LEAK")
            }
            Err(_) => false,
        };
        let detail = match &o {
            Ok(r) => format!(
                "rc={} stdout={:?} stderr={}",
                r.code,
                r.stdout.trim(),
                r.stderr.trim()
            ),
            Err(e) => e.clone(),
        };
        report(
            "C descendant inheritance (sh->sh->cat, 2 levels)",
            ok,
            &detail,
        );
        security.push(("NATIVE_C", ok));
    }

    // ---- D: same action, different plan, different outcome ---------------
    {
        let outside = q(&fx.outside_file);
        let a = sh_in(
            &fx.workspace,
            plan_for(&fx.workspace),
            &format!("cat '{outside}'"),
            &[],
        );
        let b = sh_in(
            &fx.workspace,
            SandboxPlan::Confined {
                writable_roots: vec![fx.workspace.clone()],
                readable_roots: vec![fx.outside.clone()],
                network: NetworkPolicy::Deny,
            },
            &format!("cat '{outside}'"),
            &[],
        );
        let a_denied = matches!(&a, Ok(r) if r.code != 0 && !r.stdout.contains("OUTSIDE_SECRET"));
        let b_allowed = matches!(&b, Ok(r) if r.code == 0 && r.stdout.contains("OUTSIDE_SECRET"));
        let ok = a_denied && b_allowed;
        let detail = format!(
            "planA rc={:?} | planB rc={:?}",
            a.as_ref().map(|r| r.code),
            b.as_ref().map(|r| r.code)
        );
        report("D per-action difference", ok, &detail);
        security.push(("NATIVE_D", ok));
    }

    // ---- E: network denied, with an out-of-sandbox positive control -----
    {
        if !Path::new(SYSTEM_NC).is_file() {
            report(
                "E network denied",
                false,
                &format!("BLOCKED: {SYSTEM_NC} not present on this host"),
            );
            security.push(("NATIVE_E", false));
        } else {
            // Bound by this process, outside the sandbox. `local_addr` succeeding
            // is the proof that a denial is attributable to the profile.
            let listener = match TcpListener::bind("127.0.0.1:0") {
                Ok(l) => l,
                Err(e) => {
                    report(
                        "E network denied",
                        false,
                        &format!("bind failed, so enforcement is unmeasurable: {e}"),
                    );
                    security.push(("NATIVE_E", false));
                    finish(&fx, &security);
                    return;
                }
            };
            let port = listener.local_addr().expect("local_addr").port();

            // Positive control: unsandboxed `nc` to the *same* listener. If this
            // fails, a sandboxed failure would prove nothing.
            let control = std::process::Command::new(SYSTEM_NC)
                .args(["-w", "2", "127.0.0.1", &port.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            // Drain the control connection so the listener is in a clean state.
            let _ = listener.accept();

            let script = format!(
                r#"printf 'E_STARTED_OK'; if {SYSTEM_NC} -w 2 127.0.0.1 {port} < /dev/null; then printf 'E_CONNECTED'; else printf 'E_DENIED'; fi; exit 0"#
            );
            let o = sh_in(&fx.workspace, plan_for(&fx.workspace), &script, &[]);
            let ok = control
                && matches!(&o, Ok(r) if
                    r.code == 0
                    && r.stdout.contains("E_STARTED_OK")
                    && !r.stdout.contains("E_CONNECTED")
                    && r.stdout.contains("E_DENIED"));
            let detail = match &o {
                Ok(r) => format!(
                    "listener=127.0.0.1:{port} (positive_control={}) rc={} stdout={:?} stderr={}",
                    control,
                    r.code,
                    r.stdout.trim(),
                    r.stderr.trim()
                ),
                Err(e) => e.clone(),
            };
            report("E network denied", ok, &detail);
            security.push(("NATIVE_E", ok));
        }
    }

    finish(&fx, &security);
}

/// Run the two runtime probes and emit the summary block.
fn finish(fx: &Fixture, security: &[(&str, bool)]) {
    let python_runtime = check_interpreter(fx, "python3", "-c", "print(\"OK\")");
    let node_runtime = check_interpreter(fx, "node", "-e", "console.log(\"OK\")");
    emit(security, python_runtime, node_runtime);
}

/// Whether a non-system interpreter can run under this profile (with a
/// toolchain grant). This is **runtime compatibility, not enforcement**: it is
/// reported separately from `NATIVE_A..E` and never gates the security verdict.
fn check_interpreter(
    fx: &Fixture,
    program: &str,
    runner_flag: &str,
    snippet: &str,
) -> &'static str {
    if which(program).is_none() {
        return "UNAVAILABLE";
    }
    let tc = toolchain_roots_for(program);
    // A host whose interpreter is only an xcode-select stub will be refused a
    // toolchain grant (the stub is not a regular file in a recognised prefix)
    // or will fail to run under the minimal PATH. Either way the answer is a
    // compatibility result, never a confinement result.
    let script = format!("{program} {runner_flag} '{snippet}'");
    match sh_in(&fx.workspace, plan_for(&fx.workspace), &script, &tc) {
        Ok(r) if r.code == 0 && r.stdout.trim() == "OK" => "PASS",
        Ok(r) => {
            eprintln!(
                "note: {program} run under profile rc={} stdout={:?} stderr={:?}",
                r.code,
                r.stdout.trim(),
                r.stderr.trim()
            );
            "FAIL"
        }
        Err(e) => {
            eprintln!("note: {program} could not start: {e}");
            "FAIL"
        }
    }
}

fn emit(security: &[(&str, bool)], python: &str, node: &str) {
    println!("\n--- machine-readable summary ---");
    for (key, ok) in security {
        println!("{key}={}", if *ok { "true" } else { "false" });
    }
    let all = security.iter().all(|(_, ok)| *ok);
    println!(
        "NATIVE_SECURITY_ALL_PASS={}",
        if all { "true" } else { "false" }
    );
    // Runtime compatibility is informational and never gates the verdict.
    println!("RUNTIME_PYTHON={python}");
    println!("RUNTIME_NODE={node}");
}

fn host_allows_restrictive_profiles() -> bool {
    // Only ONE probe, and it is the one verified to work on a real Terminal.
    //
    // An earlier version also required
    //   (version 1)(allow default)(deny file-read*)
    // to succeed. That was wrong: a blanket `deny file-read*` cuts off the
    // system reads `/usr/bin/true` needs to start at all, so it fails with
    // rc=134 (SIGABRT) even on a host that applies restrictive profiles
    // perfectly well. Requiring it meant the probe reported ENV_BLOCKED on
    // hosts that were in fact capable.
    //
    // The precondition we actually need is narrow: `sandbox-exec` exists, and a
    // profile that narrows something *without preventing the target from
    // starting* can be applied. `deny network*` is exactly that — it narrows,
    // and `true` still runs. If this fails, `sandbox_apply` itself failed,
    // which is the only condition that legitimately means ENV_BLOCKED.
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
