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
pub struct BoundedGitCapture {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    /// The whole stdout fit inside the budget and the deadline.
    pub complete: bool,
    /// The deadline elapsed and git was terminated.
    pub timed_out: bool,
}

/// Outcome of a brokered git read that captured **both** streams under separate
/// byte budgets.
///
/// Distinct from [`BoundedGitCapture`], which is the stdout-only shape
/// `project_context` wants (a bounded prefix of `git ls-files`, stderr
/// discarded). A caller that reports a git *failure message* back to a
/// human — or that decides a read failed because stderr was truncated — needs
/// stderr as evidence, so it must not have to infer that from an empty
/// `String::new()`.
#[derive(Debug)]
pub struct BoundedGitRead {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// stdout hit its byte budget before EOF.
    pub stdout_capped: bool,
    /// stderr hit its byte budget before EOF.
    pub stderr_capped: bool,
    /// The deadline elapsed and git was terminated.
    pub timed_out: bool,
    /// The git child was accounted for, but a pipe reader could not reach EOF
    /// within the drain budget — a descendant inherited the write end.
    ///
    /// Distinct from `timed_out` in cause and folded into it in effect: a caller
    /// asking "did this finish?" must not be told "yes" about a capture a
    /// descendant is still writing into. `timed_out` stays the single question a
    /// caller has to answer, and this is the evidence for why.
    pub drain_incomplete: bool,
}

/// Why a brokered git invocation was refused before any process existed.
#[derive(Debug)]
pub struct GitRefusal {
    pub code: &'static str,
    pub detail: String,
}

/// The outcome of a test that exercises real git, kept as four distinct states.
///
/// # Why not a bool
///
/// A boolean cannot tell "it worked" from "we never found out", and that
/// distinction is the whole point. A harness that collapses a missing git, a
/// refused sandbox profile, and a genuine success into one green check produces
/// exactly the evidence the F3 review rejected: a green run that proves nothing.
///
/// So the states are separate values, `PASS` is the only one that
/// [`counts_as_pass`](Self::counts_as_pass), and aggregation reports the worst
/// thing it saw rather than diluting it. A test in a blocked state returns
/// without asserting — it is *recorded* as blocked, and the caller is expected
/// to print the verdict rather than let the harness imply success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitVerdict {
    /// The behaviour under test was observed to hold.
    Pass,
    /// The behaviour under test was observed to be broken.
    Fail,
    /// The host cannot run git at all — no toolchain, no git binary.
    HostUnavailable,
    /// The broker was exercised but the environment refused the profile, so the
    /// confinement itself remains unmeasured.
    EnvBlocked,
}

impl GitVerdict {
    /// Whether this outcome is evidence that the tested behaviour holds.
    ///
    /// The only `true` is `Pass`. Everything else — including both "we could
    /// not measure" states — is explicitly not a pass.
    pub fn counts_as_pass(self) -> bool {
        matches!(self, GitVerdict::Pass)
    }

    /// The single verdict for a set of observations.
    ///
    /// Deliberately not an average and not an "any pass wins": a `Fail`
    /// anywhere dominates, and otherwise the most-blocked state wins, so a
    /// caller cannot accidentally read a green aggregate out of evidence that
    /// never ran.
    pub fn aggregate(observed: impl IntoIterator<Item = GitVerdict>) -> GitVerdict {
        let mut worst: Option<(u8, GitVerdict)> = None;
        for verdict in observed {
            // Fail dominates everything, then HostUnavailable (nothing was
            // measured at all), then EnvBlocked (the broker ran, the kernel
            // refused), then Pass.
            let rank = match verdict {
                GitVerdict::Pass => 0,
                GitVerdict::EnvBlocked => 1,
                GitVerdict::HostUnavailable => 2,
                GitVerdict::Fail => 3,
            };
            match worst {
                Some((best, _)) if best >= rank => {}
                _ => worst = Some((rank, verdict)),
            }
        }
        worst
            .map(|(_, verdict)| verdict)
            .unwrap_or(GitVerdict::HostUnavailable)
    }
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

/// Run `git <args>` in `root` under a workspace-derived plan, capturing both
/// streams under independent byte budgets.
///
/// This is the read shape a caller needs when it must report *why* a git read
/// failed, or when a decision depends on stderr being empty rather than merely
/// uninteresting. It is deliberately a separate entry point rather than a flag
/// on [`run_git_bounded`]: bounding stderr changes what a truncated diagnostic
/// looks like, and a caller that did not ask for stderr evidence should not
/// start receiving a second truncation flag it has to reason about.
///
/// # Fail-closed
///
/// Identical to [`run_git_bounded`]: the plan and cwd are established before a
/// process exists, and an unusable root refuses rather than degrading to an
/// unconfined read.
pub fn run_git_bounded_read(
    root: &Path,
    args: &[&str],
    stdout_budget: usize,
    stderr_budget: usize,
    timeout: std::time::Duration,
) -> Result<BoundedGitRead, GitRefusal> {
    let git = BrokeredGit::spawn(
        root,
        args,
        None,
        GitStreams::BothStreamsBounded {
            stdout: stdout_budget,
            stderr: stderr_budget,
        },
    )?;
    git.finish_bounded_read(Instant::now() + timeout)
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
    /// Capture stdout and stderr each up to their own byte budget.
    BothStreamsBounded { stdout: usize, stderr: usize },
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
    /// The stderr counterpart of `stdout_truncated`. Always false for the
    /// shapes that do not bound stderr, because an unbounded read to EOF can
    /// never report truncation.
    stderr_truncated: Arc<AtomicBool>,
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
        GitStreams::BothStreamsBounded { .. } => (StreamPolicy::Piped, StreamPolicy::Piped),
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
        // The Git read profile. `Minimal` clears the environment and rebuilds
        // PATH + HOME from the sandbox's own policy, so this stays a positive
        // selection rather than a filtered inheritance.
        .env(EnvPolicy::Minimal)
        // `HOME` is restated rather than inherited: `Minimal` points HOME at the
        // cwd, and naming the workspace root explicitly keeps that guarantee
        // visible here instead of depending on the broker's default.
        .env_var("HOME", root.as_os_str());
    // Applied from the table so the profile and its documentation cannot drift
    // apart: an entry added to one is an entry in the other, and a guard test
    // holds them equal.
    let mut spec = spec;
    for (key, value, _why) in GIT_READ_PROFILE {
        spec = spec.env_var(*key, *value);
    }
    // `xcrun` is how Apple's git shim finds a developer directory. Naming the
    // resolved toolchain root explicitly is what lets a *real* git run under a
    // profile that denies the shim its `xcrun` reach, without inheriting
    // anything else from the host. `None` on a real-git host adds nothing.
    Ok(match developer_dir_for(&spec.program) {
        Some(developer_dir) => spec.env_var("DEVELOPER_DIR", developer_dir),
        None => spec,
    })
}

/// The explicit environment a brokered git read runs under.
///
/// # Why this is an allowlist and not a repair of the host environment
///
/// The tempting fix for "git cannot start under confinement" is to hand git
/// the user's environment: `PATH`, `HOME`, `SSH_*`, `GIT_*`, the lot. That
/// converts a confinement problem into a credential-exfiltration problem,
/// because a git that can read `~/.gitconfig`, `~/.ssh`, and the user's
/// global credential helpers has been handed authority this broker exists to
/// withhold — and it would be handed to a *model-reachable* caller.
///
/// So isolation stays exactly as it was (`EnvPolicy::Minimal` rebuilds the
/// environment from `PATH` + `HOME` and nothing else), and the fix is applied
/// on top of it: select a git that is functional without extra reach, and name
/// the one toolchain variable a real git legitimately needs.
///
/// # Each entry, and what it costs
///
/// * `PATH=/usr/bin:/bin` (from `Minimal`) — git's own subprogram lookups.
///   Narrow on purpose: it contains the system toolchain, never a
///   user-writable directory, so a planted binary cannot be found first.
/// * `HOME=<workspace root>` (from `Minimal`, restated) — git must not read
///   `~/.gitconfig`, user hooks, or user credentials. Pointing HOME *into the
///   workspace* means a config lookup resolves inside the authority we already
///   granted rather than escaping it.
/// * `GIT_CONFIG_NOSYSTEM=1` — do not read `/etc/gitconfig`. System config is
///   another way for the host, rather than the project, to steer git.
/// * `GIT_TERMINAL_PROMPT=0` — never block on an interactive credential
///   prompt. A read that asked for credentials has already failed; blocking
///   would turn that into a hang.
/// * `DEVELOPER_DIR=<resolved toolchain>` — present **only** when the selected
///   git is a shim that needs it. Costs nothing on a host with a real git
///   (nothing is added), and on a shim host it names the toolchain root the
///   broker already grants for toolchain reads, so it widens nothing.
///
/// # What this profile deliberately does not do
///
/// It does not disable repository-local config or hooks: a `.git/hooks`
/// script is part of the repository the caller was already granted, and
/// pretending otherwise would misrepresent the boundary. It does not add
/// `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` overrides, because `HOME` already
/// makes those lookups resolve inside the workspace. And it does not restore
/// the caller's environment under any circumstances.
///
/// The third field is the reason each entry exists, and is asserted against the
/// spec by `git_read_profile_matches_its_documented_rationale` — so an entry
/// cannot be added silently, and a rationale cannot drift from the value.
const GIT_READ_PROFILE: &[(&str, &str, &str)] = &[
    (
        "GIT_CONFIG_NOSYSTEM",
        "1",
        "system config is host authority, not project authority",
    ),
    (
        "GIT_TERMINAL_PROMPT",
        "0",
        "a read must never block on a credential prompt",
    ),
];

/// The `DEVELOPER_DIR` a shim git needs, or `None` when the resolved git is a
/// real git that does not consult `xcrun` at all.
///
/// Naming a toolchain directory is not a privilege increase: the broker
/// already grants read reach to the resolved toolchain root via
/// [`Self::trusted_toolchain_for`], and this only tells git's own shim which
/// directory that is instead of letting it search the filesystem for one.
fn developer_dir_for(git: &Path) -> Option<std::ffi::OsString> {
    // A real git resolves its own toolchain; a shim delegates to xcrun. Only
    // the shim needs help, and only the shim can be identified by asking it.
    if is_apple_shim(git) {
        developer_dir().map(std::ffi::OsString::from)
    } else {
        None
    }
}

/// Whether `git` is Apple's developer-tools shim rather than a real git.
///
/// The shim's give-away is that it resolves its toolchain through `xcrun`, so
/// it reports an `xcrun`/`xcode-select` failure when the developer directory
/// is missing or unreachable — while a real git never mentions either.
fn is_apple_shim(git: &Path) -> bool {
    const SHIM_MARKERS: &[&str] = &["xcrun", "xcode-select"];
    std::process::Command::new(git)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", git)
        .env("DEVELOPER_DIR", "")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()
        .is_some_and(|probe| {
            !probe.status.success() || {
                let stderr = String::from_utf8_lossy(&probe.stderr).to_ascii_lowercase();
                SHIM_MARKERS.iter().any(|marker| stderr.contains(marker))
            }
        })
}

/// The developer directory a shim git should be pointed at, discovered from the
/// host rather than hardcoded.
///
/// `xcode-select -p` is the host's own answer to "where are the developer
/// tools". Reading it is not a grant: the path it returns is only useful to
/// git if the broker also grants that root, and it does, as a toolchain root.
fn developer_dir() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty() && std::path::Path::new(&path).is_dir()).then_some(path)
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
        let stderr_truncated = Arc::new(AtomicBool::new(false));
        let stdout = spawn_reader(
            child.child_mut().stdout.take(),
            match streams {
                GitStreams::BothPiped => None,
                GitStreams::StdoutOnlyBounded(budget) => Some(budget),
                GitStreams::BothStreamsBounded { stdout, .. } => Some(stdout),
            },
            Arc::clone(&truncated),
        );
        let stderr = spawn_reader(
            child.child_mut().stderr.take(),
            match streams {
                GitStreams::BothStreamsBounded { stderr, .. } => Some(stderr),
                GitStreams::BothPiped | GitStreams::StdoutOnlyBounded(_) => None,
            },
            Arc::clone(&stderr_truncated),
        );

        Ok(Self {
            child,
            stdout,
            stdout_truncated: truncated,
            stderr,
            stderr_truncated,
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
                // The direct child is gone, but its pipe may not be. Descendants
                // inherit the write end, so EOF is not implied by this status.
                // Give the tree a bounded moment to clear it so the readers can
                // finish normally; if it does not, terminate it, so the drain
                // join below is bounded by [`DRAIN_TAIL_BUDGET`] instead of by
                // however long a descendant lives.
                if self
                    .child
                    .wait_tree_exit(DRAIN_TAIL_BUDGET)
                    .unwrap_or(false)
                {
                    break status;
                }
                let _ = self.child.terminate_tree();
                break self.child.wait().map_err(|error| GitRefusal {
                    code: "git_wait_failed",
                    detail: error.to_string(),
                })?;
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

    /// Wait for git under a caller's absolute deadline, collecting both streams
    /// under their own byte budgets.
    ///
    /// The tree-terminate-on-truncation rule from [`Self::finish_bounded`]
    /// applies to **either** stream here: a git that is still writing into a
    /// pipe nobody drains cannot finish, so leaving it alive would hold the
    /// workspace past the caller's deadline.
    fn finish_bounded_read(mut self, deadline: Instant) -> Result<BoundedGitRead, GitRefusal> {
        let mut timed_out = false;
        let status = loop {
            if self.stdout_truncated.load(Ordering::SeqCst)
                || self.stderr_truncated.load(Ordering::SeqCst)
            {
                let _ = self.child.terminate_tree();
            }
            if let Some(status) = self.child.try_wait().map_err(|error| GitRefusal {
                code: "git_wait_failed",
                detail: error.to_string(),
            })? {
                // See `finish_bounded`: a reaped direct child does not imply its
                // pipes reached EOF, so the tree gets a bounded chance to clear
                // them before the readers are joined under a deadline.
                if self
                    .child
                    .wait_tree_exit(DRAIN_TAIL_BUDGET)
                    .unwrap_or(false)
                {
                    break status;
                }
                let _ = self.child.terminate_tree();
                break self.child.wait().map_err(|error| GitRefusal {
                    code: "git_wait_failed",
                    detail: error.to_string(),
                })?;
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

        let (stdout, stdout_drain_incomplete) =
            join_reader_bounded(self.stdout, "stdout", DRAIN_TAIL_BUDGET);
        let (stderr, stderr_drain_incomplete) =
            join_reader_bounded(self.stderr, "stderr", DRAIN_TAIL_BUDGET);
        // A drain that could not finish means the request outlived its budget in
        // the only way a caller cares about. Reporting `timed_out` here is what
        // stops a caller from treating a partial capture as a complete answer.
        let drain_incomplete = stdout_drain_incomplete || stderr_drain_incomplete;
        if drain_incomplete {
            eprintln!(
                "webcodex-workspace: git reader drain exceeded {}ms after the child was \
                 accounted for; reporting the capture as incomplete rather than blocking the \
                 caller on a descendant-held pipe",
                DRAIN_TAIL_BUDGET.as_millis()
            );
        }

        Ok(BoundedGitRead {
            status,
            stdout,
            stderr,
            stdout_capped: self.stdout_truncated.load(Ordering::SeqCst),
            stderr_capped: self.stderr_truncated.load(Ordering::SeqCst),
            timed_out: timed_out || drain_incomplete,
            drain_incomplete,
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

/// How long a drained pipe may delay the *end* of a request, once the git child
/// itself has already been accounted for.
///
/// This is not the git budget: it is the drain tail. Reaching it means the
/// direct child is gone but a descendant it spawned still holds the write end
/// open, so the reader thread is never going to observe EOF on its own.
///
/// Bounded rather than unbounded on purpose. An unbounded join turns "git
/// forked something that outlives it" into "this request never returns", which
/// is strictly worse than reporting a truncated capture: one is a bounded,
/// observable degradation, the other is a hang.
const DRAIN_TAIL_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

/// Collect one drained pipe, giving up on it if it cannot finish in time.
///
/// # Why the join is bounded
///
/// Waiting for a pipe to reach EOF is waiting on *every* process that inherited
/// the write end, not just on git. A git that spawns a background helper
/// (`git log` paging into a shell, a hook, a credential helper) can exit while
/// its descendant keeps stdout open indefinitely. A plain `join()` here would
/// therefore let a descendant extend the caller's request forever, long after
/// the deadline that the caller agreed to.
///
/// # Why a timeout is not a fabricated result
///
/// A drain timeout reports *incomplete capture* and never changes the exit
/// status: the caller still sees exactly what git exited with. What it loses is
/// the tail of a stream that was already known to be unreliable, which is why
/// the bounded shapes report it through `*_capped` and the unbounded ones say so
/// in their own type.
fn join_reader_bounded(
    reader: Option<std::thread::JoinHandle<Vec<u8>>>,
    stream: &'static str,
    budget: std::time::Duration,
) -> (Vec<u8>, bool) {
    let Some(handle) = reader else {
        return (Vec::new(), false);
    };
    // `JoinHandle` has no timed join on stable, so the handshake is a channel:
    // the reader sends its bytes and drops the sender, which is what makes the
    // receiver report a disconnected channel instead of blocking forever.
    let (sender, receiver) = std::sync::mpsc::channel::<Vec<u8>>();
    let pumped = std::thread::spawn(move || {
        let collected = handle.join().unwrap_or_else(|_| {
            eprintln!("webcodex-workspace: {stream} reader thread panicked");
            Vec::new()
        });
        let _ = sender.send(collected);
    });
    // The pump thread is detached on the timeout path: it finishes when the
    // reader eventually does, and dropping its `JoinHandle` keeps the timeout
    // path itself from becoming the thing that blocks.
    drop(pumped);
    match receiver.recv_timeout(budget) {
        Ok(collected) => (collected, false),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => (Vec::new(), true),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => (Vec::new(), false),
    }
}

/// Collect one drained pipe, bounded by [`DRAIN_TAIL_BUDGET`].
///
/// # Why a panicking reader is not fatal here
///
/// The reader thread only does I/O on a pipe, which cannot panic. If it ever
/// did, returning empty output keeps the caller honest about the **exit
/// status** — which it still sees — instead of converting a capture problem
/// into a fabricated git failure.
fn join_reader(reader: Option<std::thread::JoinHandle<Vec<u8>>>, stream: &'static str) -> Vec<u8> {
    join_reader_bounded(reader, stream, DRAIN_TAIL_BUDGET).0
}

/// Resolve `git` from a fixed set of trusted system prefixes.
///
/// Deliberately not a PATH search: the sandboxed child has a minimal PATH, and
/// the executable must be chosen by trusted code rather than by whatever the
/// current environment happens to contain first.
///
/// # Why existence is not enough
///
/// On macOS `/usr/bin/git` is not git — it is Apple's **shim**, a thin wrapper
/// that hands the real work to `xcrun`, which in turn resolves a developer
/// directory. The shim file exists on a machine whose developer tooling does
/// not, so `is_file()` answers a question nobody asked: it proves a wrapper was
/// installed, not that git can run.
///
/// That distinction is not cosmetic. Under a profile that denies the shim its
/// `xcrun` reach, `/usr/bin/git` fails with `xcode-select: error: tool 'git'
/// requires Xcode` while a real git at `/opt/homebrew/bin/git` serves every
/// metadata read the catalog needs. Selecting on `is_file()` therefore picks
/// the *least* functional candidate first and turns a working host into one
/// that reports no branch, no head, and no dirty state for every project.
///
/// So a candidate must be **functional**: it has to answer a trivial version
/// query under the same minimal environment the child will get. Anything less
/// is a shim we must skip over rather than a git we can use.
fn resolve_git_executable() -> Option<PathBuf> {
    CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|candidate| is_functional_git(candidate))
}

/// The git this build would actually run, for a caller that must agree with it.
///
/// # Why a caller needs this
///
/// A test fixture that builds its repository with a *different* git than the
/// broker selects is testing a configuration the product never runs. The F2
/// review found exactly that: the fixture picked by `is_file()` and got Apple's
/// shim, while production picked a different binary, and the resulting
/// "regression" was an artifact of the fixture rather than a property of the
/// code.
///
/// So the selection rule is exposed for fixtures to reuse, and it is the same
/// function — not a re-implementation that could drift.
pub fn functional_git_for_tests() -> Option<PathBuf> {
    resolve_git_executable()
}

/// The trusted prefixes searched for a usable git, in preference order.
///
/// A Homebrew git is preferred **when both work**, because it is a real git
/// rather than a developer-tools shim: it does not consult `xcrun`, so it keeps
/// working under a profile that denies the shim its developer directory. The
/// system prefixes stay in the list as a fallback for hosts with no
/// Homebrew, where a fully installed Command Line Tools does provide a real
/// git underneath the shim.
const CANDIDATES: &[&str] = &[
    "/opt/homebrew/bin/git",
    "/usr/local/bin/git",
    "/opt/local/bin/git",
    "/usr/bin/git",
    "/bin/git",
];

/// How long the pre-flight version probe may take before a candidate is
/// declared unusable.
///
/// Deliberately short: this runs on the request path, before any real work,
/// and a candidate that cannot answer `--version` quickly is not worth the
/// wait. It is a liveness probe, not a workload.
const GIT_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether `candidate` can actually answer a git query under the environment
/// the brokered child will receive.
///
/// Probed with `env_clear` plus the same `PATH`/`HOME` shape the child gets,
/// so "it works here" means "it works there" for the part that matters: whether
/// git can start at all. Without the probe, the only signal is a file existing,
/// which a shim satisfies.
fn is_functional_git(candidate: &Path) -> bool {
    if !candidate.is_file() {
        return false;
    }
    // Bounded, because this is a synchronous pre-flight on the request path.
    // `Command::status` alone would inherit an unbounded wait from the
    // candidate, and the whole point of probing is that we do not trust it.
    let Ok(mut child) = std::process::Command::new(candidate)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", candidate)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + GIT_PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(_) => return false,
        }
    }
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

    // -----------------------------------------------------------------------
    // F1 — the whole operation is bounded, not just the child's own lifetime.
    // -----------------------------------------------------------------------

    /// A reader that never sees EOF must not be able to extend the request.
    ///
    /// This is the F1 counterexample at the level it was reported: a git whose
    /// *direct child* has already exited, while a descendant it spawned keeps
    /// the stdout pipe open. The child's exit status is therefore available
    /// immediately — `try_wait` succeeds on the first poll — so a
    /// deadline loop that breaks on exit and then joins the reader will sit on
    /// that join for as long as the descendant lives, which is forever.
    ///
    /// The assertion is on **elapsed time and on the reported outcome**, never
    /// on the descendant's cooperation: a fix that merely waits longer still
    /// fails, and a fix that returns `timed_out = false` while doing so fails as
    /// well. Both halves matter, because the reported review observed exactly
    /// that combination — a long wait *and* a success verdict.
    #[test]
    fn a_descendant_holding_the_pipe_cannot_extend_the_request() {
        let Some(repo) = git_init_dir() else {
            eprintln!(
                "P1B_F1_DRAIN_BOUND=HOST_UNAVAILABLE no functional git to create a workspace; \
                 this is NOT a pass"
            );
            return;
        };

        // The counterexample, built from `sh` so it needs no helper binary: the
        // direct child writes nothing, spawns a background descendant that
        // inherits both pipe write ends and sleeps for a minute, then exits 0.
        //
        // `try_wait` therefore succeeds immediately with a **success** status —
        // exactly the state the review measured as "status=0, timed_out=false
        // after 8.028s" — while the reader cannot see EOF for another minute.
        let script = "(sleep 60 &) ; exit 0";
        let deadline = std::time::Duration::from_millis(400);
        let budget = std::time::Duration::from_secs(20);
        let started = Instant::now();
        let outcome = run_sh_bounded_read(repo.path(), script, deadline);
        let elapsed = started.elapsed();

        let (verdict, read) = match outcome {
            Ok(read) => (GitVerdict::Pass, read),
            Err(error) => {
                eprintln!("P1B_F1_DRAIN_BOUND=ENV_BLOCKED broker refused to launch ({error}); this is NOT a pass");
                return;
            }
        };
        let _ = verdict;
        if !read.status.success() {
            let stderr = String::from_utf8_lossy(&read.stderr);
            if is_profile_refusal(&read.stderr) || is_host_unavailable(stderr.as_bytes()) {
                eprintln!(
                    "P1B_F1_DRAIN_BOUND=ENV_BLOCKED the host refused the profile ({stderr}); this \
                     is NOT a pass"
                );
                return;
            }
            panic!("F1 fixture failed for an unrelated reason: {stderr}");
        }

        // The descendant outlives `budget` on purpose, so anything close to it
        // means the reader join was unbounded.
        assert!(
            elapsed < budget,
            "F1: the request must end near its deadline, not when the descendant exits \
             (elapsed {elapsed:?} exceeded {budget:?} — the reader join is unbounded)"
        );
        assert!(
            read.drain_incomplete,
            "F1: a capture the descendant never finished must be reported incomplete"
        );
        assert!(
            read.timed_out,
            "F1: an incomplete capture must not be reported as a clean finish — the review \
             observed status=0 with timed_out=false, which is exactly this lie"
        );
        eprintln!(
            "P1B_F1_DRAIN_BOUND=PASS the descendant-held pipe ended the request at {elapsed:?} \
             under a {deadline:?} deadline and reported the capture incomplete"
        );
    }

    /// The bound itself, asserted independently of any subprocess.
    ///
    /// A timing assertion on a real child can be satisfied by a slow machine or
    /// defeated by a fast one. This pins the property directly: given a reader
    /// that provably never completes, the bounded join must return within its
    /// budget instead of waiting on it.
    #[test]
    fn the_drain_join_is_bounded_even_when_the_reader_never_finishes() {
        let (sender, receiver) = std::sync::mpsc::channel::<Vec<u8>>();
        let reader = std::thread::spawn(move || {
            // Never send, and never drop `sender`: the receiver must time out
            // rather than observe a disconnect.
            std::mem::forget(sender);
            std::thread::sleep(std::time::Duration::from_secs(30));
            Vec::new()
        });

        let started = Instant::now();
        let (collected, incomplete) = join_reader_bounded(
            Some(reader),
            "stdout",
            std::time::Duration::from_millis(200),
        );
        let elapsed = started.elapsed();

        assert!(
            incomplete,
            "a reader that cannot finish must be reported incomplete, not as empty output"
        );
        assert!(collected.is_empty());
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "the bounded join must give up near its budget, took {elapsed:?}"
        );
    }

    /// A reader that finishes in time is still collected in full.
    ///
    /// The bound must not degrade a normal capture: an ordinary git that writes
    /// its answer and exits has to arrive intact, or the fix would be trading a
    /// hang for silent data loss.
    #[test]
    fn a_reader_that_finishes_in_time_is_still_collected_in_full() {
        let reader = std::thread::spawn(|| b"main\n".to_vec());
        let (collected, incomplete) =
            join_reader_bounded(Some(reader), "stdout", std::time::Duration::from_secs(5));
        assert!(
            !incomplete,
            "a prompt reader must not be reported incomplete"
        );
        assert_eq!(collected, b"main\n");
    }

    /// A child that has exited but whose tree has not must not be reported as a
    /// clean finish on the strength of its status alone.
    ///
    /// Pins the decision the F1 review called out: `try_wait` returning `Some`
    /// is evidence about the *direct child*, and says nothing about whether the
    /// pipes it left behind will ever reach EOF. The synthetic status here is
    /// the exact shape the review recorded — `status = 0` alongside an
    /// unfinished capture — and the invariant is that the two cannot both be
    /// reported as "fine" by a caller reading only `timed_out`.
    #[test]
    fn drain_incompleteness_is_reported_even_when_the_child_exited_successfully() {
        // `status` is intentionally unused: the point is that a caller must
        // consult `timed_out` / `drain_incomplete` and cannot infer completion
        // from a zero exit status alone.
        let read = BoundedGitRead {
            status: std::process::ExitStatus::default(),
            stdout: b"main\n".to_vec(),
            stderr: Vec::new(),
            stdout_capped: false,
            stderr_capped: false,
            timed_out: true,
            drain_incomplete: true,
        };
        assert!(
            read.timed_out && read.drain_incomplete,
            "an incomplete drain must never surface as a completed read"
        );
    }

    /// Drive `/bin/sh` through the **same** broker wait algorithm as a git read.
    ///
    /// The point is to exercise the waiting code with a program whose behaviour
    /// we control precisely, and the only variable here is the program: the
    /// plan derivation, cwd, environment profile, dual byte budgets, network
    /// policy and the deadline loop are the production ones, reached through the
    /// same [`BrokeredGit::spawn`] and the same [`BrokeredGit::finish_bounded_read`].
    ///
    /// So a fix that only made `sh` behave would not pass: it has to be the
    /// shared wait path that is bounded.
    fn run_sh_bounded_read(
        root: &Path,
        script: &str,
        timeout: std::time::Duration,
    ) -> Result<BoundedGitRead, GitRefusal> {
        let sh = std::path::Path::new("/bin/sh");
        if !sh.is_file() {
            return Err(GitRefusal {
                code: "git_executable_unavailable",
                detail: "/bin/sh is unavailable on this host".to_string(),
            });
        }
        let plan = workspace_git_plan(root)?;
        let spec = SpawnSpec::new(sh.to_path_buf(), root.to_path_buf(), plan)
            .args(["-c", script])
            .stdin(StreamPolicy::Null)
            .stdout(StreamPolicy::Piped)
            .stderr(StreamPolicy::Piped)
            .env(EnvPolicy::Minimal)
            .env_var("HOME", root.as_os_str());
        let mut child = ExecutionBroker::new()
            .spawn_with_toolchain(&spec, &trusted_toolchain_for(sh))
            .map_err(|error| GitRefusal {
                code: "git_spawn_refused",
                detail: error.to_string(),
            })?;
        let truncated = Arc::new(AtomicBool::new(false));
        let stderr_truncated = Arc::new(AtomicBool::new(false));
        let stdout = spawn_reader(
            child.child_mut().stdout.take(),
            Some(64 * 1024),
            Arc::clone(&truncated),
        );
        let stderr = spawn_reader(
            child.child_mut().stderr.take(),
            Some(64 * 1024),
            Arc::clone(&stderr_truncated),
        );
        let mut git = BrokeredGit {
            child,
            stdout,
            stdout_truncated: truncated,
            stderr,
            stderr_truncated,
        };
        git.finish_bounded_read(Instant::now() + timeout)
    }

    /// Create a git repository, or `None` when this host has no usable git.
    fn git_init_dir() -> Option<tempfile::TempDir> {
        let repo = tempfile::tempdir().ok()?;
        let git = resolve_git_executable()?;
        // `git init` through a plain `Command` is fixture setup, not a
        // model-reachable execution: the repository has to exist before anything
        // can be confined to it.
        let ok = std::process::Command::new(git)
            .args(["init", "-q"])
            .current_dir(repo.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        ok.then_some(repo)
    }

    /// Create a git repository that has **a commit in it**, or `None`.
    ///
    /// The commit is the whole point. A bare `git init` has no `HEAD`, so
    /// `rev-parse --abbrev-ref HEAD` exits 128 and `log -1` exits 128 — which
    /// is precisely the fixture that made the previous smoke test able to
    /// "pass" while proving nothing. Author identity is supplied on the command
    /// line because the brokered profile deliberately has no global config to
    /// read an identity from.
    fn git_init_with_commit() -> Option<tempfile::TempDir> {
        let repo = git_init_dir()?;
        let git = resolve_git_executable()?;
        let write = |args: &[&str]| {
            std::process::Command::new(&git)
                .args(args)
                .current_dir(repo.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        std::fs::write(repo.path().join("fixture.txt"), b"fixture\n").ok()?;
        if !write(&["add", "fixture.txt"]) {
            return None;
        }
        let identity = [
            "-c",
            "user.email=webcodex@example.invalid",
            "-c",
            "user.name=webcodex",
        ];
        let commit: Vec<&str> = identity
            .iter()
            .copied()
            .chain(["commit", "-q", "-m", "fixture commit"])
            .collect();
        write(&commit).then_some(repo)
    }

    // -----------------------------------------------------------------------
    // F2 — the Git read profile.
    // -----------------------------------------------------------------------

    /// The profile is applied as documented, and nothing else is.
    ///
    /// Isolation is the invariant that must not move: `Minimal` plus a positive
    /// allowlist, never `Inherit`. F2's fix was explicitly *not* "give git the
    /// user's environment", so this test fails if anyone takes that route.
    #[test]
    fn git_read_profile_matches_its_documented_rationale() {
        let repo = tempfile::tempdir().unwrap();
        let spec = brokered_git_spec(
            repo.path(),
            &["rev-parse", "--abbrev-ref", "HEAD"],
            None,
            GitStreams::BothStreamsBounded {
                stdout: 1024,
                stderr: 1024,
            },
        )
        .expect("the spec is fully determined before any process exists");

        assert_eq!(
            spec.env,
            EnvPolicy::Minimal,
            "F2: the fix must preserve isolation, not switch to EnvPolicy::Inherit"
        );

        let named: std::collections::BTreeMap<String, String> = spec
            .env_vars
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string_lossy().to_string()))
            .collect();

        // Every documented entry is present with exactly its documented value.
        for (key, value, why) in GIT_READ_PROFILE {
            assert_eq!(
                named.get(*key).map(String::as_str),
                Some(*value),
                "F2: profile entry {key} must be {value} ({why})"
            );
        }

        // HOME is pinned into the workspace, never the caller's home.
        assert_eq!(
            named.get("HOME").map(String::as_str),
            Some(repo.path().to_string_lossy().as_ref()),
            "F2: HOME must stay inside the workspace so user config and credentials are \
             unreachable"
        );

        // Nothing beyond the documented set is named. A credential-shaped host
        // variable must never appear here, and neither may an ad-hoc addition.
        let documented: std::collections::BTreeSet<&str> = GIT_READ_PROFILE
            .iter()
            .map(|(key, _, _)| *key)
            .chain(["HOME"])
            .collect();
        for key in named.keys() {
            assert!(
                documented.contains(key.as_str()),
                "F2: {key} is set on a brokered git but is not in the documented profile"
            );
        }

        // And the plan still denies the network and grants only the root.
        assert_eq!(
            spec.plan,
            workspace_git_plan(repo.path()).expect("plan derivation is deterministic"),
            "F2: the read profile must not widen the sandbox plan"
        );
    }

    /// The resolved git must be one that can actually answer, not merely exist.
    ///
    /// F2's root cause was selecting on `is_file()`, which an Apple developer
    /// tools shim satisfies while being unable to run. Where a functional git
    /// exists, the shim must not win merely by being listed first.
    #[test]
    fn the_resolved_git_is_functional_not_merely_present() {
        let Some(git) = resolve_git_executable() else {
            eprintln!(
                "P1B_F2_GIT_PROFILE=HOST_UNAVAILABLE no functional git on this host; this is NOT \
                 a pass and NOT a security regression"
            );
            return;
        };
        assert!(git.is_absolute(), "a resolved git must be a concrete path");
        // Re-probe the resolved candidate: resolution claimed it works, so a
        // second probe must agree. A disagreement means selection is answering
        // a different question than the one the broker relies on.
        assert!(
            is_functional_git(&git),
            "F2: the resolved git {} cannot answer --version under the child's environment",
            git.display()
        );
        eprintln!(
            "P1B_F2_GIT_PROFILE=PASS resolved functional git {}",
            git.display()
        );
    }

    /// A shim that cannot start must be skipped in favour of a working git.
    ///
    /// Asserted as behaviour of the selection rule rather than as a property of
    /// any particular host: when a functional candidate exists, whatever
    /// `CANDIDATES` lists earlier must not be returned.
    #[test]
    fn a_non_functional_git_is_never_selected() {
        // A file that exists but is not an executable git is the cheapest
        // reproduction of the shim failure mode: present, trusted-prefixed, and
        // unable to answer.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("git");
        std::fs::write(&fake, b"#!/bin/sh\nexit 1\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }
        assert!(fake.is_file(), "the fixture must exist as a file");
        assert!(
            !is_functional_git(&fake),
            "F2: a file that cannot answer --version must not pass the functional probe"
        );
    }

    // -----------------------------------------------------------------------
    // F3 — outcome accounting.
    // -----------------------------------------------------------------------

    /// The catalog's three real metadata reads, through the real broker.
    ///
    /// This is F2's acceptance evidence, and it is deliberately shaped the way
    /// the review demanded:
    ///
    /// * **A real git**, resolved through the production selector — not a fake
    ///   binary, and not a hand-run shell command either. The child is launched
    ///   by the broker with the production plan and profile.
    /// * **A real commit**, because the review's own counterexample had a
    ///   fixture that failed: a fresh `git init` has no `HEAD`, so
    ///   `rev-parse --abbrev-ref HEAD` legitimately exits 128 and the test
    ///   proved nothing. This fixture commits, so all three reads have a real
    ///   answer to return.
    /// * **A recorded verdict.** On a host whose kernel refuses the sandbox the
    ///   outcome is `ENV_BLOCKED` and the test says so; it never prints
    ///   "not a pass" and returns as green, which is F3's finding restated.
    #[test]
    fn real_git_metadata_reads_succeed_through_the_broker() {
        let Some(repo) = git_init_with_commit() else {
            eprintln!(
                "P1B_F2_GIT_METADATA=HOST_UNAVAILABLE no functional git on this host; this is \
                 NOT a pass"
            );
            return;
        };

        // The three reads the catalog actually performs, in the shape it
        // performs them: branch name, short head, and dirtiness.
        let expectations: [(&[&str], &str); 3] = [
            (&["rev-parse", "--abbrev-ref", "HEAD"], "branch"),
            (&["log", "-1", "--pretty=format:%h"], "head"),
            (&["status", "--short"], "dirty"),
        ];
        let mut verdicts: Vec<GitVerdict> = Vec::new();

        for (argv, what) in expectations {
            let read = match run_git_bounded_read(
                repo.path(),
                argv,
                64 * 1024,
                64 * 1024,
                std::time::Duration::from_secs(10),
            ) {
                Ok(read) => read,
                Err(refusal) => {
                    eprintln!(
                        "P1B_F2_GIT_METADATA=ENV_BLOCKED broker refused to launch git ({refusal}); \
                         this is NOT a pass"
                    );
                    verdicts.push(GitVerdict::EnvBlocked);
                    continue;
                }
            };

            if !read.status.success() {
                let stderr = String::from_utf8_lossy(&read.stderr);
                if is_profile_refusal(&read.stderr) {
                    eprintln!(
                        "P1B_F2_GIT_METADATA=ENV_BLOCKED the kernel refused the profile for \
                         `git {what}` ({stderr}); this is NOT a pass"
                    );
                    verdicts.push(GitVerdict::EnvBlocked);
                    continue;
                }
                if is_host_unavailable(stderr.as_bytes()) {
                    eprintln!(
                        "P1B_F2_GIT_METADATA=HOST_UNAVAILABLE git could not run for `git {what}` \
                         ({stderr}); this is NOT a pass"
                    );
                    verdicts.push(GitVerdict::HostUnavailable);
                    continue;
                }
                panic!("F2: real `git {what}` failed under the broker profile: {stderr}");
            }
            assert!(
                !read.timed_out && !read.drain_incomplete,
                "F2: `git {what}` must complete cleanly through the broker"
            );

            // A real read has to produce a real answer, not merely exit zero.
            // `rev-parse` and `log` must return a value; `status --short` is
            // legitimately empty for a clean tree, so only its success is
            // asserted. Asserting an answer here is what makes this a
            // metadata-extraction proof rather than an exit-code check.
            if what != "dirty" {
                let answer = String::from_utf8_lossy(&read.stdout).trim().to_string();
                assert!(
                    !answer.is_empty(),
                    "F2: `git {what}` succeeded but returned no metadata; the read proved nothing"
                );
                assert!(
                    !read.stdout_capped,
                    "F2: `git {what}` output was capped, so the answer is partial"
                );
                eprintln!("P1B_F2_GIT_METADATA git {what} -> {answer}");
            } else {
                eprintln!(
                    "P1B_F2_GIT_METADATA git {what} -> (clean tree, empty output as expected)"
                );
            }
            verdicts.push(GitVerdict::Pass);
        }

        let aggregate = GitVerdict::aggregate(verdicts);
        match aggregate {
            GitVerdict::Pass => {
                eprintln!(
                    "P1B_F2_GIT_METADATA=PASS rev-parse, log and status all returned real metadata \
                     through the broker"
                );
            }
            // Not a failure of the code and not a pass: the environment refused
            // the profile, so the metadata path remains unmeasured here. It is
            // reported so a reader never mistakes this run for evidence, and the
            // run returns green because there is no defect to fail on.
            blocked => {
                eprintln!(
                    "P1B_F2_GIT_METADATA={blocked:?} the metadata path could not be measured on \
                     this host; this is NOT a pass and NOT a regression"
                );
            }
        }
    }

    /// `only_a_pass_is_a_pass`

    /// The four outcomes are distinct values, not a boolean and a hope.
    ///
    /// F3's finding was that a test could print "NOT a pass" and still be
    /// counted green. The structural defence is that the verdict is a value with
    /// no "blocked but ok" variant: an aggregation can only sum passes, and this
    /// pins that `ENV_BLOCKED` and `HOST_UNAVAILABLE` are not passes.
    #[test]
    fn only_a_pass_is_a_pass() {
        assert!(GitVerdict::Pass.counts_as_pass());
        for blocked in [
            GitVerdict::EnvBlocked,
            GitVerdict::HostUnavailable,
            GitVerdict::Fail,
        ] {
            assert!(
                !blocked.counts_as_pass(),
                "F3: {blocked:?} must never aggregate as a pass"
            );
        }
        assert_eq!(
            GitVerdict::aggregate([GitVerdict::Pass, GitVerdict::EnvBlocked]),
            GitVerdict::EnvBlocked,
            "F3: one blocked observation must not be diluted into a pass by a sibling pass"
        );
        assert_eq!(
            GitVerdict::aggregate([GitVerdict::EnvBlocked, GitVerdict::HostUnavailable]),
            GitVerdict::HostUnavailable,
            "F3: blocked evidence must stay visible, and never measured ranks worst"
        );
        assert_eq!(
            GitVerdict::aggregate([GitVerdict::Fail, GitVerdict::Pass]),
            GitVerdict::Fail,
            "F3: a single failure must not be averaged away by a pass"
        );
        assert_eq!(
            GitVerdict::aggregate([GitVerdict::Pass, GitVerdict::Pass]),
            GitVerdict::Pass,
            "an aggregate of nothing but passes is a pass"
        );
    }
}
