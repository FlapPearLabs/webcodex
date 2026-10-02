// SPDX-License-Identifier: Apache-2.0
//! Git execution for workspace inspection and checkpoints, routed through the
//! P1 execution broker.
//!
//! # Why git runs under the broker
//!
//! `git` is model-reachable in two ways that matter. `workspace_checkpoint`
//! accepts patch content on stdin and writes workspace state, so an unconfined
//! `git` here is an unconfined write primitive. And `git` itself shells out to
//! pagers, hooks and credential helpers, so "this is just a read" is not a
//! safe assumption about what the process tree does.
//!
//! Both callers therefore hand their **trusted** project root to [`run_git`] or
//! [`run_git_bounded`], which derive the sandbox plan from that root and refuse
//! if the root is not usable. There is no fallback to a bare `Command::spawn`:
//! an unusable root means no checkpoint and no project context, which is the
//! correct outcome for a boundary that cannot be established.
//!
//! # What is and is not model-chosen
//!
//! The caller chooses argv and, in some cases, stdin bytes. It does not choose
//! the sandbox root: that is the trusted project root it was given. The model
//! cannot widen authority by asking for a different directory.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use webcodex_process::execution_broker::{
    EnvPolicy, ExecutionBroker, NetworkPolicy, SandboxPlan, SpawnSpec, StreamPolicy,
    TrustedToolchainRoot,
};

/// Outcome of one brokered git invocation.
#[derive(Debug)]
pub(crate) struct GitOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// Outcome of a brokered git invocation whose stdout was captured under a byte
/// budget and a deadline.
#[derive(Debug)]
pub(crate) struct BoundedGitCapture {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    /// The whole stdout fit inside the budget and the deadline.
    pub(crate) complete: bool,
    /// The deadline elapsed and git was terminated.
    pub(crate) timed_out: bool,
}

/// Why a brokered git invocation was refused before any process existed.
#[derive(Debug)]
pub(crate) struct GitRefusal {
    pub(crate) code: &'static str,
    pub(crate) detail: String,
}

impl std::fmt::Display for GitRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

/// Build the P1 workspace-derived plan for a trusted project root.
///
/// The root must be an absolute, existing directory. `readable_roots` is empty
/// on purpose: the workspace is already readable as a writable root, and naming
/// no additional root is what keeps git from gaining reach outside the project.
pub(crate) fn workspace_git_plan(root: &Path) -> Result<SandboxPlan, GitRefusal> {
    if !root.is_absolute() {
        return Err(GitRefusal {
            code: "git_workspace_root_invalid",
            detail: format!(
                "project root {} is not absolute; refusing to derive a sandbox authority",
                root.display()
            ),
        });
    }
    let canonical = root.canonicalize().map_err(|error| GitRefusal {
        code: "git_workspace_root_invalid",
        detail: format!(
            "project root {} does not resolve to a real directory: {error}",
            root.display()
        ),
    })?;
    if !canonical.is_dir() {
        return Err(GitRefusal {
            code: "git_workspace_root_invalid",
            detail: format!("project root {} is not a directory", canonical.display()),
        });
    }
    Ok(SandboxPlan::Confined {
        writable_roots: vec![canonical],
        readable_roots: Vec::new(),
        network: NetworkPolicy::Deny,
    })
}

/// Run `git <args>` in `root` under a workspace-derived plan, collecting all of
/// its output.
///
/// # Fail-closed
///
/// The plan is compiled and the cwd is checked *before* a process exists, so a
/// bad root, an inexpressible plan, or a cwd outside the authority all refuse
/// rather than degrading to an unconfined child.
pub(crate) fn run_git(
    root: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    timeout: Option<std::time::Duration>,
) -> Result<GitOutput, GitRefusal> {
    let git = BrokeredGit::spawn(root, args, input, GitStreams::BothPiped)?;
    git.finish(timeout)
}

/// Run `git <args>` capturing at most `max_bytes` of stdout, abandoning the run
/// at `deadline`.
///
/// This is `project_context`'s shape: it wants a *bounded prefix* of a
/// potentially enormous `git ls-files` and treats truncation as a partial
/// result rather than a failure. The broker owns the profile; the byte budget
/// and the deadline stay the caller's policy.
pub(crate) fn run_git_bounded(
    root: &Path,
    args: &[&str],
    max_bytes: usize,
    deadline: Instant,
) -> Result<BoundedGitCapture, GitRefusal> {
    let git = BrokeredGit::spawn(root, args, None, GitStreams::StdoutOnlyBounded(max_bytes))?;
    git.finish_bounded(deadline)
}

/// Which of git's streams the caller needs.
///
/// Stated explicitly because `project_context` reads a bounded prefix of
/// stdout and discards stderr entirely, and because an inherited stderr would
/// leak the runner's own stderr into a git that is supposed to be quiet.
#[derive(Debug, Clone, Copy)]
enum GitStreams {
    /// Capture all of stdout and all of stderr.
    BothPiped,
    /// Capture stdout up to a byte budget; discard stderr.
    StdoutOnlyBounded(usize),
}

/// A git child that already exists under a workspace-confined profile.
///
/// The handle deliberately cannot widen authority: nothing on it changes the
/// plan, the cwd, the environment, or the resolved executable.
struct BrokeredGit {
    child: webcodex_process::ManagedChild,
    stdout: Option<std::thread::JoinHandle<Vec<u8>>>,
    /// Set when a bounded reader stopped before EOF, so the caller can tell a
    /// complete capture from a truncated one.
    stdout_truncated: Arc<AtomicBool>,
    stderr: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl std::fmt::Debug for BrokeredGit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokeredGit")
            .field("pid", &self.child.id())
            .finish()
    }
}

/// Build the exact `SpawnSpec` a brokered git child would be launched with,
/// without starting anything.
///
/// Split out from [`BrokeredGit::spawn`] so that the *authority* of a brokered
/// git — its program, plan, cwd, and above all its environment — can be
/// asserted directly by tests, including on a host whose kernel refuses the
/// profile outright. On such a host the launch itself is `ENV_BLOCKED` and
/// establishes nothing, but the spec is still fully determined beforehand, so
/// "does not inherit the caller's environment" remains provable without it.
fn brokered_git_spec(
    root: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    streams: GitStreams,
) -> Result<SpawnSpec, GitRefusal> {
    let plan = workspace_git_plan(root)?;

    // `git` must be resolvable from a fixed, trusted prefix: a bare `"git"`
    // would be resolved through the sandboxed child's PATH, which the plan
    // does not grant. Resolving it here, in the trusted parent, keeps the
    // executable choice outside model influence.
    let git = resolve_git_executable().ok_or_else(|| GitRefusal {
        code: "git_executable_unavailable",
        detail: "git could not be resolved from a trusted system prefix".to_string(),
    })?;

    let (stdout_policy, stderr_policy) = match streams {
        GitStreams::BothPiped => (StreamPolicy::Piped, StreamPolicy::Piped),
        GitStreams::StdoutOnlyBounded(_) => (StreamPolicy::Piped, StreamPolicy::Null),
    };

    Ok(SpawnSpec::new(git, root.to_path_buf(), plan)
        .args(args.iter().map(std::ffi::OsString::from))
        // An explicit input payload is piped; its absence is an explicit EOF
        // source. Never inherit the caller's stdin, which may be a
        // parent-liveness pipe.
        .stdin(if input.is_some() {
            StreamPolicy::Piped
        } else {
            StreamPolicy::Null
        })
        .stdout(stdout_policy)
        .stderr(stderr_policy)
        // A minimal environment: `PATH` for git's own subprogram lookups, and
        // a `HOME` inside the workspace so git cannot read the user's global
        // config, credentials, or hooks from `$HOME`.
        .env(EnvPolicy::Minimal)
        .env_var("HOME", root.as_os_str())
        .env_var("GIT_CONFIG_NOSYSTEM", "1")
        .env_var("GIT_TERMINAL_PROMPT", "0"))
}

impl BrokeredGit {
    /// Spawn `git <args>` in `root` under the workspace-derived plan.
    fn spawn(
        root: &Path,
        args: &[&str],
        input: Option<&[u8]>,
        streams: GitStreams,
    ) -> Result<Self, GitRefusal> {
        let spec = brokered_git_spec(root, args, input, streams)?;

        let mut child = ExecutionBroker::new()
            .spawn_with_toolchain(&spec, &trusted_toolchain_for(&spec.program))
            .map_err(|error| GitRefusal {
                code: "git_spawn_refused",
                detail: error.to_string(),
            })?;

        // The payload must be written *before* the readers start and before the
        // wait, then the pipe closed: `git apply` blocks until its patch
        // arrives or stdin reaches EOF.
        if let Some(input) = input {
            let mut child_stdin = child.child_mut().stdin.take().ok_or_else(|| GitRefusal {
                code: "git_stdin_unavailable",
                detail: "git stdin pipe missing".to_string(),
            })?;
            child_stdin.write_all(input).map_err(|error| GitRefusal {
                code: "git_stdin_write_failed",
                detail: error.to_string(),
            })?;
        }

        let truncated = Arc::new(AtomicBool::new(false));
        let stdout = spawn_reader(
            child.child_mut().stdout.take(),
            match streams {
                GitStreams::BothPiped => None,
                GitStreams::StdoutOnlyBounded(budget) => Some(budget),
            },
            Arc::clone(&truncated),
        );
        let stderr = spawn_reader(
            child.child_mut().stderr.take(),
            None,
            Arc::new(AtomicBool::new(false)),
        );

        Ok(Self {
            child,
            stdout,
            stdout_truncated: truncated,
            stderr,
        })
    }

    /// Wait for git, honouring an optional timeout, and collect all of its output.
    ///
    /// On timeout the tree is terminated rather than abandoned: a git that
    /// outlives its budget would keep holding the workspace.
    fn finish(mut self, timeout: Option<std::time::Duration>) -> Result<GitOutput, GitRefusal> {
        let status = self.wait_for(timeout)?;
        Ok(GitOutput {
            status,
            stdout: join_reader(self.stdout, "stdout"),
            stderr: join_reader(self.stderr, "stderr"),
        })
    }

    /// Wait for git under a caller's absolute deadline and report whether the
    /// bounded capture was complete.
    fn finish_bounded(mut self, deadline: Instant) -> Result<BoundedGitCapture, GitRefusal> {
        let mut timed_out = false;
        let status = loop {
            if self.stdout_truncated.load(Ordering::SeqCst) {
                // The reader stopped early, which means git still has output to
                // write into a pipe nobody is draining. Terminate rather than
                // wait on a child that cannot finish.
                let _ = self.child.terminate_tree();
            }
            if let Some(status) = self.child.try_wait().map_err(|error| GitRefusal {
                code: "git_wait_failed",
                detail: error.to_string(),
            })? {
                break status;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                let _ = self.child.terminate_tree();
                break self.child.wait().map_err(|error| GitRefusal {
                    code: "git_wait_failed",
                    detail: error.to_string(),
                })?;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };

        let stdout = join_reader(self.stdout, "stdout");
        let truncated = self.stdout_truncated.load(Ordering::SeqCst);
        Ok(BoundedGitCapture {
            status,
            complete: !timed_out && !truncated,
            timed_out,
            stdout,
        })
    }

    /// Reap the direct child, terminating the tree first if `timeout` elapses.
    fn wait_for(&mut self, timeout: Option<std::time::Duration>) -> Result<ExitStatus, GitRefusal> {
        let Some(timeout) = timeout else {
            return self.child.wait().map_err(|error| GitRefusal {
                code: "git_wait_failed",
                detail: error.to_string(),
            });
        };
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = self.child.terminate_tree();
                        return self.child.wait().map_err(|error| GitRefusal {
                            code: "git_timeout_wait_failed",
                            detail: format!("git exceeded its {timeout:?} budget: {error}"),
                        });
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => {
                    return Err(GitRefusal {
                        code: "git_wait_failed",
                        detail: error.to_string(),
                    })
                }
            }
        }
    }
}

/// Drain one pipe to EOF on a dedicated thread, optionally under a byte budget.
///
/// Draining before waiting is not an optimisation: a git subprocess can emit
/// more than a pipe buffer holds and then block on write, so a caller that
/// waits first and reads afterwards can deadlock against its own child.
fn spawn_reader<R: Read + Send + 'static>(
    pipe: Option<R>,
    budget: Option<usize>,
    truncated: Arc<AtomicBool>,
) -> Option<std::thread::JoinHandle<Vec<u8>>> {
    let mut pipe = pipe?;
    Some(std::thread::spawn(move || {
        let mut buffer = Vec::new();
        match budget {
            // Unbounded: read to EOF. A read error ends collection, but the exit
            // status remains the authority on whether the command succeeded, so
            // a truncated capture is never reported as a git failure.
            None => {
                let _ = pipe.read_to_end(&mut buffer);
            }
            Some(limit) => {
                let mut chunk = [0u8; 16 * 1024];
                loop {
                    // Read at most one byte past the budget: that is how
                    // truncation is detected without buffering the whole stream.
                    let remaining = limit.saturating_sub(buffer.len());
                    let wanted = chunk.len().min(remaining.saturating_add(1));
                    if wanted == 0 {
                        truncated.store(true, Ordering::SeqCst);
                        return buffer;
                    }
                    match pipe.read(&mut chunk[..wanted]) {
                        Ok(0) => return buffer,
                        Ok(read) => {
                            let keep = read.min(remaining);
                            buffer.extend_from_slice(&chunk[..keep]);
                            if read > remaining {
                                truncated.store(true, Ordering::SeqCst);
                                return buffer;
                            }
                        }
                        Err(ref error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => return buffer,
                    }
                }
            }
        }
        buffer
    }))
}

/// Collect one drained pipe.
///
/// # Why a panicking reader is not fatal here
///
/// The reader thread only does I/O on a pipe, which cannot panic. If it ever
/// did, returning empty output keeps the caller honest about the **exit
/// status** — which it still sees — instead of converting a capture problem
/// into a fabricated git failure.
fn join_reader(reader: Option<std::thread::JoinHandle<Vec<u8>>>, stream: &'static str) -> Vec<u8> {
    match reader {
        Some(handle) => handle.join().unwrap_or_else(|_| {
            eprintln!("webcodex-workspace: {stream} reader thread panicked");
            Vec::new()
        }),
        None => Vec::new(),
    }
}

/// Resolve `git` from a fixed set of trusted system prefixes.
///
/// Deliberately not a PATH search: the sandboxed child has a minimal PATH, and
/// the executable must be chosen by trusted code rather than by whatever the
/// current environment happens to contain first.
fn resolve_git_executable() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "/usr/bin/git",
        "/bin/git",
        "/usr/local/bin/git",
        "/opt/homebrew/bin/git",
        "/opt/local/bin/git",
    ];
    CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|candidate| candidate.is_file())
}

/// Read-only prefixes the child needs to execute the git binary itself.
///
/// Only recognised toolchain prefixes can become a grant, so this can widen
/// *read* reach to a toolchain directory and nothing else — in particular not
/// to `/`, and not to the user's home.
fn trusted_toolchain_for(program: &Path) -> Vec<TrustedToolchainRoot> {
    TrustedToolchainRoot::resolve(program).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_grants_only_the_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let writable_roots;
        let readable_roots;
        let network;
        match workspace_git_plan(dir.path()).unwrap() {
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
        assert_eq!(writable_roots, vec![dir.path().canonicalize().unwrap()]);
        assert!(
            readable_roots.is_empty(),
            "git must gain no extra readable roots"
        );
        assert_eq!(network, NetworkPolicy::Deny);
    }

    #[test]
    fn relative_root_is_refused() {
        let err = workspace_git_plan(Path::new("relative")).unwrap_err();
        assert_eq!(err.code, "git_workspace_root_invalid");
    }

    #[test]
    fn missing_root_is_refused() {
        let err = workspace_git_plan(Path::new("/webcodex/definitely/not/here")).unwrap_err();
        assert_eq!(err.code, "git_workspace_root_invalid");
    }

    #[test]
    fn a_file_is_not_a_git_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        let err = workspace_git_plan(&file).unwrap_err();
        assert_eq!(err.code, "git_workspace_root_invalid");
    }

    #[test]
    fn brokered_git_never_inherits_the_callers_environment() {
        // The property under test is that a brokered git does not inherit
        // arbitrary host variables. It is asserted on the spec rather than on a
        // live child so that it holds on a host where the kernel refuses the
        // profile: `EnvPolicy::Minimal` clears the environment and rebuilds it
        // from `PATH` + `HOME` alone, so nothing else can survive regardless of
        // what the parent process happened to hold.
        let dir = tempfile::tempdir().unwrap();

        // An arbitrary host variable, of the shape a real credential takes. It
        // is set in *this* process so that if the spec inherited anything, this
        // is the value that would show up.
        std::env::set_var("GH_TOKEN", "should_not_reach_git");
        std::env::set_var("WEBCODEX_P1B_PROBE_SECRET", "should_not_reach_git");

        let spec = brokered_git_spec(
            dir.path(),
            &["ls-files", "-z"],
            None,
            GitStreams::StdoutOnlyBounded(1024),
        )
        .expect("the spec is fully determined before any process exists");

        // `Minimal` is the whole point: it is what makes the environment a
        // positive selection rather than a denylist.
        assert_eq!(spec.env, EnvPolicy::Minimal);

        let named: Vec<&str> = spec.env_vars.iter().map(|(key, _)| key.as_str()).collect();
        for key in &named {
            assert!(
                !key.contains("GH_TOKEN"),
                "a credential-shaped host variable must never be named: {key}"
            );
            assert!(
                !key.contains("WEBCODEX_P1B_PROBE_SECRET"),
                "an arbitrary host variable must never be named: {key}"
            );
        }

        // The positively-selected keys are exactly the hardened git contract.
        // `env_vars` is an ordered map, so compare as a set.
        let mut expected = vec!["HOME", "GIT_CONFIG_NOSYSTEM", "GIT_TERMINAL_PROMPT"];
        expected.sort_unstable();
        let mut got = named.clone();
        got.sort_unstable();
        assert_eq!(
            got, expected,
            "only the hardened git environment may be named, got {named:?}"
        );
        let home = spec
            .env_vars
            .iter()
            .find(|(key, _)| key.as_str() == "HOME")
            .map(|(_, value)| value.clone())
            .expect("HOME must be pinned inside the workspace");
        assert_eq!(
            home.as_os_str(),
            dir.path().as_os_str(),
            "HOME must point at the workspace, never the caller's home directory"
        );

        let _ = std::env::remove_var("GH_TOKEN");
        let _ = std::env::remove_var("WEBCODEX_P1B_PROBE_SECRET");
    }

    #[test]
    fn resolved_git_is_never_a_bare_name() {
        // Resolution must produce a concrete path, because the sandboxed child
        // has a minimal PATH and could not resolve a bare name.
        if let Some(git) = resolve_git_executable() {
            assert!(git.is_absolute());
            assert!(git.is_file());
        }
    }

    #[test]
    fn toolchain_grant_never_covers_the_whole_filesystem() {
        if let Some(git) = resolve_git_executable() {
            for root in trusted_toolchain_for(&git) {
                assert!(
                    root.as_path() != Path::new("/"),
                    "a toolchain grant must never become the filesystem root"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // P1-G / P1-H: the git surface, against the real broker path.
    // -----------------------------------------------------------------------

    /// Initialize `root` as a git repository with the trusted git, returning
    /// `false` when this host has no usable git.
    ///
    /// Fixture setup, not model-triggered execution: the repository has to
    /// exist before the broker can confine anything to it.
    fn git_init(root: &Path) -> bool {
        let Some(git) = resolve_git_executable() else {
            return false;
        };
        std::process::Command::new(git)
            .args(["init", "-q"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// **P1-G, broker half.** `git apply` still applies a real patch with git
    /// patch semantics unchanged, through the broker.
    ///
    /// This is **fidelity only**: it drives `git_broker::run_git` directly. It
    /// does not touch `workspace_checkpoint::git_apply`, so it cannot speak for
    /// the checkpoint-wrapper path the model reaches in production — that half
    /// is covered by `workspace_checkpoint`'s own
    /// `checkpoint_git_apply_applies_a_real_patch_through_the_broker`. The
    /// verdict therefore carries its own marker
    /// (`P1_NATIVE_GIT_BROKER_FIDELITY`), never `P1_NATIVE_GIT_APPLY`, so a
    /// passing broker test can never be mistaken for evidence about the
    /// checkpoint layer.
    ///
    /// The gate is **routing plus fidelity**, not confinement. Non-applying runs
    /// come from three different places and must not be confused:
    ///
    /// * The launcher itself refuses to start (`git_spawn_refused`) — the
    ///   fail-closed path, a statement about the broker.
    /// * The launcher starts but the kernel refuses the profile, so `sandbox-exec`
    ///   exits non-zero with `sandbox_apply: Operation not permitted`. That is
    ///   `ENV_BLOCKED`: the environment, not the code.
    /// * git never runs because the host toolchain is incomplete — no developer
    ///   tools, `xcode-select` missing, no git binary. That is
    ///   `HOST_UNAVAILABLE`: the broker was never exercised, so it is neither a
    ///   pass nor a security regression.
    ///
    /// Only git starting and rejecting the patch is `FAIL`.
    #[test]
    fn g_git_apply_still_applies_a_real_patch_through_the_broker() {
        let repo = tempfile::tempdir().unwrap();
        if !git_init(repo.path()) {
            eprintln!("P1_GIT_APPLY_REASON=no_trusted_git_executable");
            eprintln!(
                "P1_NATIVE_GIT_BROKER_FIDELITY=HOST_UNAVAILABLE no trusted git executable on this \
                 host; this is NOT a pass and NOT a security regression"
            );
            return;
        }

        let patch = concat!(
            "diff --git a/added.txt b/added.txt\n",
            "new file mode 100644\n",
            "index 0000000..9daeafb\n",
            "--- /dev/null\n",
            "+++ b/added.txt\n",
            "@@ -0,0 +1 @@\n",
            "+applied-through-broker\n",
        );
        let argv = vec!["apply", "-"];
        match run_git(repo.path(), &argv, Some(patch.as_bytes()), None) {
            Ok(output) => {
                if output.status.success() {
                    // Emit the machine-readable verdict *before* the assertion, so
                    // a genuine mismatch still shows up in the smoke log.
                    eprintln!("P1_GIT_APPLY_REASON=patch_applied");
                    eprintln!(
                        "P1_NATIVE_GIT_BROKER_FIDELITY=PASS git apply applied the patch through \
                         the broker (lower-level fidelity only; NOT the checkpoint-wrapper path)"
                    );
                    assert_eq!(
                        std::fs::read_to_string(repo.path().join("added.txt")).unwrap(),
                        "applied-through-broker\n",
                        "git patch semantics must be preserved through the broker"
                    );
                    return;
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                if is_profile_refusal(&output.stderr) {
                    eprintln!("P1_GIT_APPLY_REASON=sandbox_profile_refused");
                    eprintln!(
                        "P1_NATIVE_GIT_BROKER_FIDELITY=ENV_BLOCKED broker launched but the kernel \
                         refused the profile ({stderr}); this is NOT a pass"
                    );
                    assert!(
                        !repo.path().join("added.txt").exists(),
                        "a refused profile must not have applied the patch"
                    );
                    return;
                }
                // git ran and refused the patch. Before calling that a security
                // failure, rule out an incomplete host toolchain: a stub `git`
                // with no developer tools behind it fails here while the broker
                // behaved correctly, and blaming confinement for that would be
                // backwards.
                if is_host_unavailable(stderr.as_bytes()) {
                    eprintln!("P1_GIT_APPLY_REASON=host_toolchain_unavailable");
                    eprintln!(
                        "P1_NATIVE_GIT_BROKER_FIDELITY=HOST_UNAVAILABLE git could not run because \
                         the host toolchain is incomplete ({stderr}); this is NOT a pass and NOT \
                         a security regression"
                    );
                    assert!(
                        !repo.path().join("added.txt").exists(),
                        "a failed apply must not leave the patch applied"
                    );
                    return;
                }
                eprintln!("P1_GIT_APPLY_REASON=patch_rejected_by_git");
                eprintln!("P1_NATIVE_GIT_BROKER_FIDELITY=FAIL git apply failed: {stderr}");
                panic!("P1-G git apply failed on an unconfined-capable host: {stderr}");
            }
            // The launcher never started a process: fail-closed, and still not a
            // statement about patch semantics.
            Err(refusal) => {
                if refusal.code == "git_executable_unavailable" {
                    eprintln!("P1_GIT_APPLY_REASON=git_executable_unavailable");
                    eprintln!(
                        "P1_NATIVE_GIT_BROKER_FIDELITY=HOST_UNAVAILABLE no trusted git executable \
                         ({refusal}); this is NOT a pass and NOT a security regression"
                    );
                    return;
                }
                eprintln!("P1_GIT_APPLY_REASON=broker_refused_to_spawn");
                eprintln!(
                    "P1_NATIVE_GIT_BROKER_FIDELITY=ENV_BLOCKED broker refused to launch git ({refusal})"
                );
                assert_eq!(refusal.code, "git_spawn_refused");
            }
        }
    }

    /// Whether git's stderr shows the sandbox launcher refusing the profile
    /// rather than git itself failing.
    fn is_profile_refusal(stderr: &[u8]) -> bool {
        let text = String::from_utf8_lossy(stderr);
        text.contains("sandbox_apply") || text.contains("sandbox-exec")
    }

    /// Whether git's stderr shows the host toolchain is incomplete, as opposed
    /// to git running and rejecting the patch.
    ///
    /// On macOS a machine without developer tools still has the `/usr/bin/git`
    /// shim, and it fails with `xcode-select: error: tool 'git' requires Xcode`.
    /// No repository is touched, no patch is read, and no confinement decision is
    /// made — so this is a property of the machine and is classified as
    /// HOST_UNAVAILABLE rather than blamed on the broker.
    ///
    /// `not a git repository` is deliberately excluded: git emitted it, so git
    /// ran, and it is telling us the workspace was outside a repository — which
    /// is the invariant under test, not a missing tool.
    ///
    /// `git_executable_unavailable` is excluded for a different reason: that is
    /// a `GitRefusal` *code*, not something git writes to stderr. It is matched
    /// on the refusal itself, where the broker reports never having started a
    /// process, so it is classified there rather than inferred from text here.
    fn is_host_unavailable(stderr: &[u8]) -> bool {
        let text = String::from_utf8_lossy(stderr);
        text.contains("xcode-select")
            || text.contains("requires Xcode")
            || text.contains("No developer tools")
            || text.contains("no developer tools")
            || text.contains("unable to locate developer tools")
            || text.contains("Cannot find a developer tool")
    }

    /// **P1-H** The git helper gains no authority beyond the project root.
    ///
    /// Asserted against the real plan function, so it holds on hosts that
    /// cannot apply a profile.
    #[test]
    fn h_git_helper_plan_grants_only_the_project_root() {
        let repo = tempfile::tempdir().unwrap();
        let writable_roots;
        let readable_roots;
        let network;
        match workspace_git_plan(repo.path()).unwrap() {
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
        assert_eq!(writable_roots, vec![repo.path().canonicalize().unwrap()]);
        assert!(readable_roots.is_empty(), "git gains no extra read roots");
        assert_eq!(network, NetworkPolicy::Deny);
    }

    /// **P1-C / P1-J, git half**: an unusable project root refuses before a
    /// process exists, rather than falling back to a bare spawn.
    #[test]
    fn unusable_root_refuses_before_spawn() {
        let missing = Path::new("/webcodex/definitely/not/here");
        let refusal = run_git(missing, &["status"], None, None).expect_err("must refuse");
        assert_eq!(refusal.code, "git_workspace_root_invalid");
    }

    /// The HOST_UNAVAILABLE / FAIL boundary is the whole point of the four-state
    /// classification, so it is pinned rather than left to a comment. Widening
    /// `is_host_unavailable` is easy and quiet; this is what makes it fail loudly.
    #[test]
    fn host_unavailable_never_swallows_a_security_or_env_signal() {
        // Host gaps: the toolchain is missing, so nothing was measured. These
        // are the shapes macOS actually emits — the `/usr/bin/git` shim exists
        // and fails at `xcode-select` when developer tools are absent.
        for missing in [
            "xcode-select: error: tool 'git' requires Xcode",
            "xcode-select: error: unable to locate developer tools",
            "No developer tools were found, install them and retry",
        ] {
            assert!(
                is_host_unavailable(missing.as_bytes()),
                "must be HOST_UNAVAILABLE: {missing}"
            );
        }

        // git ran and told us something about the workspace. These are the
        // invariant under test, or an environment refusal, and neither may be
        // filed as a missing tool.
        for security in [
            "fatal: not a git repository (or any of the parent directories): .git",
            "error: pathspec 'x.txt' did not match any file(s) known to git",
            "fatal: patch failed: checkpoint-added.txt: No such file or directory",
        ] {
            assert!(
                !is_host_unavailable(security.as_bytes()),
                "must NOT be HOST_UNAVAILABLE, or a real defect hides as a host gap: {security}"
            );
        }

        // The profile refusal belongs to the ENV_BLOCKED axis, not this one.
        assert!(is_profile_refusal(
            b"sandbox-exec: sandbox_apply: Operation not permitted"
        ));
        assert!(!is_host_unavailable(
            b"sandbox-exec: sandbox_apply: Operation not permitted"
        ));
    }
}
