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

impl BrokeredGit {
    /// Spawn `git <args>` in `root` under the workspace-derived plan.
    fn spawn(
        root: &Path,
        args: &[&str],
        input: Option<&[u8]>,
        streams: GitStreams,
    ) -> Result<Self, GitRefusal> {
        let plan = workspace_git_plan(root)?;

        // `git` must be resolvable from a fixed, trusted prefix: a bare `"git"`
        // would be resolved through the sandboxed child's PATH, which the plan
        // does not grant. Resolving it here, in the trusted parent, keeps the
        // executable choice outside model influence.
        let git = resolve_git_executable().ok_or_else(|| GitRefusal {
            code: "git_executable_unavailable",
            detail: "git could not be resolved from a trusted system prefix".to_string(),
        })?;

        let (stdout_policy, stderr_policy, bounded) = match streams {
            GitStreams::BothPiped => (StreamPolicy::Piped, StreamPolicy::Piped, None),
            GitStreams::StdoutOnlyBounded(budget) => {
                (StreamPolicy::Piped, StreamPolicy::Null, Some(budget))
            }
        };

        let spec = SpawnSpec::new(git, root.to_path_buf(), plan)
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
            .env_var("GIT_TERMINAL_PROMPT", "0");

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
            bounded,
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

    /// **P1-G** `git apply` still applies a real patch with git patch semantics
    /// unchanged, through the broker.
    ///
    /// The gate is **routing plus fidelity**, not confinement. Two hosts
    /// produce a non-applying run and they must not be confused:
    ///
    /// * The launcher itself refuses to start (`git_spawn_refused`) — the
    ///   fail-closed path.
    /// * The launcher starts but the kernel refuses the profile, so `sandbox-exec`
    ///   exits non-zero with `sandbox_apply: Operation not permitted`. That is
    ///   `ENV_BLOCKED`: the environment, not the code. Reporting it as a git
    ///   failure would blame the patch semantics for a host limitation.
    #[test]
    fn g_git_apply_still_applies_a_real_patch_through_the_broker() {
        let repo = tempfile::tempdir().unwrap();
        if !git_init(repo.path()) {
            eprintln!("P1-G SKIPPED: no trusted git executable on this host");
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
                    eprintln!(
                        "P1_NATIVE_GIT_APPLY=PASS git apply applied the patch through the broker"
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
                    eprintln!(
                        "P1_NATIVE_GIT_APPLY=ENV_BLOCKED broker launched but the kernel \
                         refused the profile ({stderr}); this is NOT a pass"
                    );
                    assert!(
                        !repo.path().join("added.txt").exists(),
                        "a refused profile must not have applied the patch"
                    );
                    return;
                }
                eprintln!("P1_NATIVE_GIT_APPLY=FAIL git apply failed: {stderr}");
                panic!("P1-G git apply failed on an unconfined-capable host: {stderr}");
            }
            // The launcher never started a process: fail-closed, and still not a
            // statement about patch semantics.
            Err(refusal) => {
                eprintln!(
                    "P1_NATIVE_GIT_APPLY=ENV_BLOCKED broker refused to launch git ({refusal})"
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
}
