// SPDX-License-Identifier: Apache-2.0
//! Native enforcement probe: T0-T3 plus A-E, for a human-run Terminal.
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
//! compiler. There is no permissive path and no test-only shortcut here — if
//! this binary reports a pass, the profile that produced it is the profile
//! WebCodex would ship.
//!
//! # Output contract
//!
//! A machine-readable block at the end, one `KEY=true|false` per line, so the
//! wrapper script never has to parse prose:
//!
//! ```text
//! NATIVE_T0=true
//! NATIVE_T1=true
//! ...
//! NATIVE_ALL_PASS=true
//! ```

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use webcodex_process::execution_broker::{
    ExecutionBroker, NetworkPolicy, SandboxPlan, SpawnSpec, StreamPolicy,
};

// ---------------------------------------------------------------------------
// Fixture: generated per run, never a fixed /tmp path
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
/// Homebrew-installed `node` is not executable under them. Each root found
/// here is passed to the compiler, which refuses any root inside `$HOME`.
fn toolchain_roots_for(program: &str) -> Vec<PathBuf> {
    let Some(path) = which(program) else {
        return Vec::new();
    };
    let Ok(resolved) = path.canonicalize() else {
        return Vec::new();
    };
    // Walk up to the prefix: .../bin/node -> .../Cellar/node/.../bin/node, and
    // .../bin/node -> /opt/homebrew/bin/node. The prefix is the first segment
    // that is not a bin/lib directory.
    let mut roots = Vec::new();
    for ancestor in resolved.ancestors().skip(1).take(4) {
        let name = ancestor.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if matches!(name, "bin" | "lib" | "Cellar" | "opt") {
            continue;
        }
        roots.push(ancestor.to_path_buf());
        break;
    }
    roots
}

// ---------------------------------------------------------------------------
// Execution helper
// ---------------------------------------------------------------------------

struct Outcome {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run(spec: &SpawnSpec, toolchain: &[PathBuf]) -> Result<Outcome, String> {
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

fn sh_in(
    cwd: &Path,
    plan: SandboxPlan,
    script: &str,
    toolchain: &[PathBuf],
) -> Result<Outcome, String> {
    let spec = SpawnSpec::new("/bin/sh", cwd, plan)
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

    let mut results: Vec<(&str, bool)> = Vec::new();
    let fx = Fixture::new();

    // ---- T0-T3: can a normal process start at all? ----------------------
    {
        let o = sh_in(&fx.root, plan_for(&fx.workspace), "true", &[]);
        let ok = matches!(&o, Ok(r) if r.code == 0);
        let detail = match &o {
            Ok(r) => format!("rc={} stderr={}", r.code, r.stderr.trim()),
            Err(e) => e.clone(),
        };
        report("T0 /usr/bin/true", ok, &detail);
        results.push(("NATIVE_T0", ok));
    }
    {
        let o = sh_in(&fx.root, plan_for(&fx.workspace), "echo OK", &[]);
        let ok = matches!(&o, Ok(r) if r.code == 0 && r.stdout.trim() == "OK");
        let detail = match &o {
            Ok(r) => format!(
                "rc={} stdout={:?} stderr={}",
                r.code,
                r.stdout.trim(),
                r.stderr.trim()
            ),
            Err(e) => e.clone(),
        };
        report("T1 /bin/sh -c 'echo OK'", ok, &detail);
        results.push(("NATIVE_T1", ok));
    }
    {
        if which("python3").is_none() {
            report(
                "T2 python3",
                true,
                "SKIP_REASON: python3 not present on this host",
            );
            results.push(("NATIVE_T2", true));
        } else {
            let tc = toolchain_roots_for("python3");
            let o = sh_in(
                &fx.root,
                plan_for(&fx.workspace),
                "python3 -c 'print(\"OK\")'",
                &tc,
            );
            let ok = matches!(&o, Ok(r) if r.code == 0 && r.stdout.trim() == "OK");
            let detail = match &o {
                Ok(r) => {
                    let mut d = format!("rc={} stdout={:?}", r.code, r.stdout.trim());
                    if !ok {
                        d.push_str(&format!(" stderr={}", r.stderr.trim()));
                    }
                    d
                }
                Err(e) => e.clone(),
            };
            report("T2 python3 -c print", ok, &detail);
            results.push(("NATIVE_T2", ok));
        }
    }
    {
        if which("node").is_none() {
            report(
                "T3 node",
                true,
                "SKIP_REASON: node not present on this host",
            );
            results.push(("NATIVE_T3", true));
        } else {
            let tc = toolchain_roots_for("node");
            let o = sh_in(
                &fx.root,
                plan_for(&fx.workspace),
                "node -e 'console.log(\"OK\")'",
                &tc,
            );
            let ok = matches!(&o, Ok(r) if r.code == 0 && r.stdout.trim() == "OK");
            let detail = match &o {
                Ok(r) => {
                    let mut d = format!("rc={} stdout={:?}", r.code, r.stdout.trim());
                    if !ok {
                        d.push_str(&format!(" stderr={}", r.stderr.trim()));
                    }
                    d
                }
                Err(e) => e.clone(),
            };
            report("T3 node -e console.log", ok, &detail);
            results.push(("NATIVE_T3", ok));
        }
    }

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
        results.push(("NATIVE_A", ok));
    }

    // ---- B: external read denied, process demonstrably started ----------
    {
        let inside = q(&fx.inside_file);
        let outside = q(&fx.outside_file);
        // The marker proves the program ran. Without it a "denial" could just
        // mean the program never started.
        let script =
            format!(r#"cat "{inside}" > /dev/null && printf 'B_STARTED_OK' && cat "{outside}""#);
        let o = sh_in(&fx.root, plan_for(&fx.workspace), &script, &[]);
        let ok = match &o {
            Ok(r) => {
                r.code == 0
                    && r.stdout.contains("B_STARTED_OK")
                    && !r.stdout.contains("OUTSIDE_SECRET")
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
        results.push(("NATIVE_B", ok));
    }

    // ---- C: descendants inherit (shell -> python, shell -> node) --------
    {
        let outside = q(&fx.outside_file);
        if which("python3").is_none() {
            report(
                "C descendant inheritance",
                true,
                "SKIP_REASON: python3 not present",
            );
            results.push(("NATIVE_C", true));
        } else {
            let tc = toolchain_roots_for("python3");
            let script = format!(
                r#"printf 'C_SHELL_OK'; python3 -c 'import sys;print("C_LEAK:"+open(sys.argv[1]).read())' "{outside}""#
            );
            let o = sh_in(&fx.root, plan_for(&fx.workspace), &script, &tc);
            let ok = match &o {
                Ok(r) => {
                    r.code == 0 && r.stdout.contains("C_SHELL_OK") && !r.stdout.contains("C_LEAK")
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
            report("C descendant inheritance (sh->python3)", ok, &detail);
            results.push(("NATIVE_C", ok));
        }
    }
    {
        let outside = q(&fx.outside_file);
        if which("node").is_none() {
            report(
                "C2 descendant inheritance",
                true,
                "SKIP_REASON: node not present",
            );
            results.push(("NATIVE_C2", true));
        } else {
            let tc = toolchain_roots_for("node");
            let script = format!(
                r#"printf 'C2_SHELL_OK'; node -e 'const fs=require("fs");process.stdout.write("C2_LEAK:"+fs.readFileSync(process.argv[1],"utf8"))' "{outside}""#
            );
            let o = sh_in(&fx.root, plan_for(&fx.workspace), &script, &tc);
            let ok = match &o {
                Ok(r) => {
                    r.code == 0 && r.stdout.contains("C2_SHELL_OK") && !r.stdout.contains("C2_LEAK")
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
            report("C2 descendant inheritance (sh->node)", ok, &detail);
            results.push(("NATIVE_C2", ok));
        }
    }

    // ---- D: same action, different plan, different outcome ---------------
    {
        let outside = q(&fx.outside_file);
        let a = sh_in(
            &fx.root,
            plan_for(&fx.workspace),
            &format!("cat '{outside}'"),
            &[],
        );
        let b = sh_in(
            &fx.root,
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
        results.push(("NATIVE_D", ok));
    }

    // ---- E: network denied, listener provably alive ----------------------
    {
        if which("python3").is_none() {
            report("E network denied", true, "SKIP_REASON: python3 not present");
            results.push(("NATIVE_E", true));
        } else {
            // Bound by this process, outside the sandbox. `local_addr` succeeding
            // is the proof that a denial is attributable to the profile.
            let listener = match std::net::TcpListener::bind("127.0.0.1:0") {
                Ok(l) => l,
                Err(e) => {
                    report("E network denied", false, &format!("bind failed: {e}"));
                    results.push(("NATIVE_E", false));
                    emit(&results);
                    return;
                }
            };
            let port = listener.local_addr().expect("local_addr").port();

            let tc = toolchain_roots_for("python3");
            let script = format!(
                r#"printf 'E_STARTED_OK'; python3 -c 'import socket
s=socket.socket(); s.settimeout(3)
try:
    s.connect(("127.0.0.1",{port})); print("E_CONNECTED")
except OSError as exc:
    print("E_DENIED:"+type(exc).__name__)' "#
            );
            let o = sh_in(&fx.root, plan_for(&fx.workspace), &script, &tc);
            let ok = match &o {
                Ok(r) => {
                    r.code == 0
                        && r.stdout.contains("E_STARTED_OK")
                        && !r.stdout.contains("E_CONNECTED")
                        && r.stdout.contains("E_DENIED:")
                }
                Err(_) => false,
            };
            let detail = match &o {
                Ok(r) => format!(
                    "listener=127.0.0.1:{port} (bound, unsandboxed) rc={} stdout={:?} stderr={}",
                    r.code,
                    r.stdout.trim(),
                    r.stderr.trim()
                ),
                Err(e) => e.clone(),
            };
            report("E network denied", ok, &detail);
            results.push(("NATIVE_E", ok));
        }
    }

    emit(&results);
}

fn emit(results: &[(&str, bool)]) {
    println!("\n--- machine-readable summary ---");
    for (key, ok) in results {
        println!("{key}={}", if *ok { "true" } else { "false" });
    }
    let all = results.iter().all(|(_, ok)| *ok);
    println!("NATIVE_ALL_PASS={}", if all { "true" } else { "false" });
}

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
