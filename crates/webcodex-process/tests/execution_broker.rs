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
    ExecutionBroker, NetworkPolicy, SandboxPlan, SpawnSpec, StreamPolicy, TrustedToolchainRoot,
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
    let spec = SpawnSpec::new("/bin/sh", &fx.root, plan_workspace_only(&fx.workspace))
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
// ---------------------------------------------------------------------------

#[test]
fn test_c_descendant_inherits_the_profile_shell_to_python() {
    require_enforcement_capable_host!("C");

    let fx = Fixture::new();
    let inside = sh_quote_path(&fx.inside_file);
    let outside = sh_quote_path(&fx.outside_file);

    if which("python3").is_none() {
        eprintln!("SKIP_REASON[C]: python3 not present on this host");
        return;
    }

    // sh prints a marker (so we know the shell ran), then python3 runs as a
    // descendant, catches the denial itself, and exits 0. See test B for why a
    // non-zero exit must not be the oracle.
    let script = format!(
        r#"printf 'C_SHELL_OK'; python3 -c 'import sys
try:
    d = open(sys.argv[1]).read()
    print("C_LEAK:" + d)
    sys.exit(1)
except OSError:
    print("C_DENIED")
    sys.exit(0)' "{outside}""#
    );
    let spec = SpawnSpec::new("/bin/sh", &fx.root, plan_workspace_only(&fx.workspace))
        .arg("-c")
        .arg(&script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    let _ = inside;

    let (out, err, code) = run(&spec);
    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        code,
        0,
        "C: shell and python must both start; stderr={}",
        String::from_utf8_lossy(&err)
    );
    assert!(
        text.contains("C_SHELL_OK"),
        "C: the outer shell must have started; stdout={text:?}"
    );
    assert!(
        text.contains("C_DENIED"),
        "C: the descendant must observe and report the denial; stdout={text:?}"
    );
    assert!(
        !text.contains("C_LEAK"),
        "C: the descendant must not read the denied file; stdout={text:?}"
    );
}

#[test]
fn test_c2_descendant_inherits_the_profile_shell_to_node() {
    require_enforcement_capable_host!("C2");

    let fx = Fixture::new();
    let outside = sh_quote_path(&fx.outside_file);

    let node = match which("node") {
        Some(n) => n,
        None => {
            eprintln!("SKIP_REASON[C2]: node not present on this host");
            return;
        }
    };
    let _ = node;

    // Same shape as C: the descendant catches the denial and exits 0. Braces
    // are doubled because this is a `format!` template.
    let script = format!(
        r#"printf 'C2_SHELL_OK'; node -e 'const fs=require("fs");
try {{
  const d = fs.readFileSync(process.argv[1], "utf8");
  process.stdout.write("C2_LEAK:" + d);
  process.exit(1);
}} catch (e) {{
  process.stdout.write("C2_DENIED");
  process.exit(0);
}}' "{outside}""#
    );
    let spec = SpawnSpec::new("/bin/sh", &fx.root, plan_workspace_only(&fx.workspace))
        .arg("-c")
        .arg(&script)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);

    let (out, err, code) = run(&spec);
    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        code,
        0,
        "C2: shell and node must both start; stderr={}",
        String::from_utf8_lossy(&err)
    );
    assert!(
        text.contains("C2_SHELL_OK"),
        "C2: the outer shell must have started; stdout={text:?}"
    );
    assert!(
        text.contains("C2_DENIED"),
        "C2: the descendant must observe and report the denial; stdout={text:?}"
    );
    assert!(
        !text.contains("C2_LEAK"),
        "C2: the descendant must not read the denied file; stdout={text:?}"
    );
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
        let spec = SpawnSpec::new("/bin/cat", &fx.root, plan)
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
// E — network is denied, and the listener provably exists
// ---------------------------------------------------------------------------

#[test]
fn test_e_network_denied() {
    require_enforcement_capable_host!("E");

    if which("python3").is_none() {
        eprintln!("SKIP_REASON[E]: python3 not present on this host");
        return;
    }

    // Listener is created and bound by this process, OUTSIDE the sandbox, so a
    // connection failure is attributable to the profile and not to a missing
    // server. `peer_addr` succeeding proves the socket is live before the child
    // runs, which is the part round 2 could not distinguish.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    assert!(
        listener.local_addr().is_ok(),
        "listener must be bound before the child runs"
    );

    let script = format!(
        r#"printf 'E_STARTED_OK'; python3 -c 'import socket,sys
s=socket.socket(); s.settimeout(3)
try:
    s.connect(("127.0.0.1",{port})); print("E_CONNECTED")
except OSError as e:
    print("E_DENIED:"+type(e).__name__)' "#
    );
    let spec = SpawnSpec::new(
        "/bin/sh",
        &fx_workspace(),
        SandboxPlan::read_write_in(&[std::env::temp_dir()]),
    )
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
        text.contains("E_DENIED:"),
        "E: the denial must be an OS-level error, not a silent hang; stdout={text:?}"
    );
}

/// Absolute temp dir, canonicalized: E's plan must name a real, existing root.
fn fx_workspace() -> PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
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
