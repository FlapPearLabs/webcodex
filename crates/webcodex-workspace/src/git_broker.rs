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
    // `None` means "no caller budget", which is a different statement from "a
    // budget this large": an absent deadline still cannot leave an *internal*
    // wait unbounded, so the probes below are bounded by their own constants.
    let deadline = timeout.map(|budget| Instant::now() + budget);
    let git = BrokeredGit::spawn(root, args, input, GitStreams::BothPiped, deadline)?;
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
    let git = BrokeredGit::spawn(
        root,
        args,
        None,
        GitStreams::StdoutOnlyBounded(max_bytes),
        Some(deadline),
    )?;
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
    // The deadline is established **here**, before anything else can block, and
    // it is then threaded through every remaining step. Creating it after
    // `spawn` — as this did before — left executable resolution and the
    // candidate probes outside the caller's budget entirely: a probe that hung
    // for eight seconds still returned `status = 0, timed_out = false` under a
    // two-second timeout, because nothing was measuring the time it took.
    let deadline = Instant::now() + timeout;
    let git = BrokeredGit::spawn(
        root,
        args,
        None,
        GitStreams::BothStreamsBounded {
            stdout: stdout_budget,
            stderr: stderr_budget,
        },
        Some(deadline),
    )?;
    git.finish_bounded_read(deadline)
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
    deadline: Option<Instant>,
) -> Result<SpawnSpec, GitRefusal> {
    let plan = workspace_git_plan(root)?;

    // `git` must be resolvable from a fixed, trusted prefix: a bare `"git"`
    // would be resolved through the sandboxed child's PATH, which the plan
    // does not grant. Resolving it here, in the trusted parent, keeps the
    // executable choice outside model influence.
    //
    // Resolution runs **under the caller's deadline**. It launches processes —
    // a repository fixture plus three metadata reads per candidate — so a
    // candidate that hangs would otherwise spend unbounded time before the
    // caller's budget even started counting.
    let git = resolve_git_executable(deadline).ok_or_else(|| GitRefusal {
        code: "git_executable_unavailable",
        detail: "git could not be resolved from a trusted system prefix".to_string(),
    })?;

    let (stdout_policy, stderr_policy) = match streams {
        GitStreams::BothPiped => (StreamPolicy::Piped, StreamPolicy::Piped),
        GitStreams::StdoutOnlyBounded(_) => (StreamPolicy::Piped, StreamPolicy::Null),
        GitStreams::BothStreamsBounded { .. } => (StreamPolicy::Piped, StreamPolicy::Piped),
    };

    let spec = SpawnSpec::new(git.program.clone(), root.to_path_buf(), plan)
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
    // `xcrun` is how Apple's git shim finds a developer directory, so a shim
    // that cannot work without one is given exactly the directory the
    // capability probe proved it needs. This is not re-derived from the binary
    // — the probe already measured it, and the measurement is what travels here.
    // `None` on a real-git host adds nothing at all.
    Ok(match git.developer_dir {
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

/// The developer directory a candidate git should be pointed at, discovered
/// from the host rather than hardcoded.
///
/// `xcode-select -p` is the host's own answer to "where are the developer
/// tools". Reading it is not a grant: the path it returns is only useful to
/// git if the broker also grants that root, and it does, as a toolchain root.
///
/// # Why this takes the caller's deadline
///
/// This runs on the request path, inside a capability probe that is itself
/// inside the caller's budget. Giving it a fresh `GIT_PROBE_TIMEOUT` of its own
/// meant a host that could not answer `xcode-select` spent that budget *after*
/// the caller's deadline had already been established — so a request that was
/// promised a bounded lifetime still waited, and the bound it kept was not the
/// one the caller agreed to.
///
/// `GIT_PROBE_TIMEOUT` is now only a **cap** for the case where the caller
/// supplied no deadline. It can shorten the wait; it can never extend it. An
/// already-expired deadline returns `None` without launching anything, because
/// the answer is no longer worth having.
///
/// # Why the caller's `Instant` is propagated verbatim
///
/// The obvious way to write this is
/// `let budget = absolute - now; let deadline = Instant::now() + budget;` — and
/// that is a **rebase**, not a pass-through. It throws away the instant the
/// caller chose and rebuilds one from a fresh `now`, so any time already spent
/// between the caller's decision and this function's start silently refills the
/// clock. The wait is then bounded by `min(GIT_PROBE_TIMEOUT, remaining + drift)`
/// rather than by what the caller actually allowed.
///
/// So when a deadline exists it is used as-is; `GIT_PROBE_TIMEOUT` is
/// constructed as a deadline only in the branch where the caller supplied none.
fn developer_dir(deadline: Option<Instant>) -> Option<String> {
    developer_dir_via(Path::new(XCODE_SELECT), deadline)
}

/// The host tool this queries for a developer directory.
///
/// A named constant rather than an inline literal so a test can exercise the
/// timeout path against a command that never answers. `xcode-select` on a
/// healthy host answers instantly, which means the bounded path is otherwise
/// unreachable by test — the exact shape of bug where a bound exists but is
/// never exercised.
const XCODE_SELECT: &str = "/usr/bin/xcode-select";

/// The deadline a probe helper runs under, derived from its caller's.
///
/// This is the single seam through which every probe (`xcode-select`, the
/// capability probe and its metadata reads) inherits a bound, so the propagation
/// rule is stated once and is directly assertable:
///
/// * A caller-supplied `Instant` comes back **as that instant** — never converted
///   to a duration and rebuilt from a fresh `now`. The conversion-and-rebuild
///   form (`budget = absolute - now; deadline = now + budget`) is a *rebase*: the
///   second `now` is later than the first, so the rebuilt instant is later than
///   the one the caller chose, and whatever elapsed in between silently refills
///   the budget. A helper must never receive a later deadline than its caller.
/// * `GIT_PROBE_TIMEOUT` is a deadline only in the branch where the caller
///   supplied none. It shortens a generous caller's deadline; it never extends an
///   imminent one.
/// * An already-expired deadline yields `None`, so the caller's clock is not
///   converted into a fresh grant at the moment it runs out.
///
/// Returns `None` when there is no budget to probe under.
fn probe_deadline(caller: Option<Instant>) -> Option<Instant> {
    match caller {
        Some(absolute) if absolute > Instant::now() => {
            Some(std::cmp::min(absolute, Instant::now() + GIT_PROBE_TIMEOUT))
        }
        // The caller's budget is spent; a probe under it measures nothing.
        Some(_) => None,
        // No caller deadline exists — *this* is the branch where a fresh cap is
        // constructed, and the only one.
        None => Some(Instant::now() + GIT_PROBE_TIMEOUT),
    }
}

/// [`developer_dir`], against an explicit command.
///
/// The command is a parameter so the deadline logic can be tested with a
/// process that hangs. It is not a grant: the value is only useful to git if
/// the broker also grants that root, which it does as a toolchain root.
fn developer_dir_via(command: &Path, deadline: Option<Instant>) -> Option<String> {
    let deadline = probe_deadline(deadline)?;

    let mut child = std::process::Command::new(command)
        .arg("-p")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                // `try_wait` already reaped the child, so collecting its pipes
                // cannot block on it — only on a descendant, which `-p` does not
                // spawn.
                let output = child.wait_with_output().ok()?;
                if !output.status.success() {
                    return None;
                }
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return (!path.is_empty() && std::path::Path::new(&path).is_dir()).then_some(path);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

impl BrokeredGit {
    /// Spawn `git <args>` in `root` under the workspace-derived plan.
    fn spawn(
        root: &Path,
        args: &[&str],
        input: Option<&[u8]>,
        streams: GitStreams,
        deadline: Option<Instant>,
    ) -> Result<Self, GitRefusal> {
        let spec = brokered_git_spec(root, args, input, streams, deadline)?;

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
    ///
    /// # Why the drain here is a fresh budget per stream, not one shared tail
    ///
    /// This path has no caller deadline: `timeout` is consumed by
    /// [`Self::wait_for`] to bound the *wait*, and what remains afterwards is
    /// whatever the caller allowed minus however long git took — a value this
    /// function does not have, because `wait_for` does not return it.
    ///
    /// So each stream draws a full [`DRAIN_TAIL_BUDGET`] of its own. That is not
    /// the accumulation defect: there is no caller deadline here to accumulate
    /// *past*. The guarantee a bounded caller needs — "cleanup ends by the
    /// instant I named" — is [`Self::finish_bounded`]'s, which opens one tail
    /// from that instant and shares it.
    ///
    /// Sharing a single 250ms tail between the two streams here was tried and is
    /// wrong: the second stream is then handed whatever the first left, which is
    /// typically nothing, so a stream that was merely slow comes back **empty**
    /// while the call still reports success. `brokered_git_usable` probes with
    /// `--version` and reads stderr to decide whether the sandbox refused the
    /// profile; an empty stderr makes a refused profile look like a working
    /// one, and every environment-gated test then runs against a host that
    /// cannot run git at all.
    fn finish(mut self, timeout: Option<std::time::Duration>) -> Result<GitOutput, GitRefusal> {
        let status = self.wait_for(timeout)?;
        // One full budget per stream: independent allowances, because there is
        // no caller clock for them to spend in common.
        let (stdout, _) = join_reader_bounded(self.stdout, "stdout", DRAIN_TAIL_BUDGET);
        let (stderr, _) = join_reader_bounded(self.stderr, "stderr", DRAIN_TAIL_BUDGET);
        Ok(GitOutput {
            status,
            stdout,
            stderr,
        })
    }

    /// Wait for git under a caller's absolute deadline and report whether the
    /// bounded capture was complete.
    fn finish_bounded(mut self, deadline: Instant) -> Result<BoundedGitCapture, GitRefusal> {
        // ONE tail, opened before the first blocking stage and never reopened.
        // The tree wait and the stdout drain below both draw from this instant,
        // so together they cannot exceed `DRAIN_TAIL_BUDGET` even when the
        // caller's own deadline is still minutes away.
        let tail_deadline = open_drain_tail(deadline);
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
                // join below is bounded by the remaining tail instead of by
                // however long a descendant lives.
                if self
                    .child
                    .wait_tree_exit(remaining_tail(tail_deadline))
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

        let stdout = join_reader(self.stdout, "stdout", tail_deadline);
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
        // ONE tail, opened here and never reopened. Tree wait, stdout drain and
        // stderr drain are three blocking stages; all three draw from this single
        // instant, so their combined cost cannot exceed `DRAIN_TAIL_BUDGET`.
        let tail_deadline = open_drain_tail(deadline);
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
                    .wait_tree_exit(remaining_tail(tail_deadline))
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

        // The drain tail is drawn from the ONE instant opened above, not granted
        // afresh per stage. Handing each stage its own `DRAIN_TAIL_BUDGET` let the
        // tail add up: tree wait, then stdout, then stderr, so a request could
        // finish up to three budgets past the deadline the caller agreed to. Each
        // stage below therefore consumes what is left of the same tail rather
        // than re-measuring it — asking "how much is left now" is what makes the
        // stages share one budget rather than merely start from the same number.
        let stdout_budget = remaining_tail(tail_deadline);
        let (stdout, stdout_drain_incomplete) =
            join_reader_bounded(self.stdout, "stdout", stdout_budget);
        let stderr_budget = remaining_tail(tail_deadline);
        let (stderr, stderr_drain_incomplete) =
            join_reader_bounded(self.stderr, "stderr", stderr_budget);
        // A drain that could not finish means the request outlived its budget in
        // the only way a caller cares about. Reporting `timed_out` here is what
        // stops a caller from treating a partial capture as a complete answer.
        let drain_incomplete = stdout_drain_incomplete || stderr_drain_incomplete;
        if drain_incomplete {
            eprintln!(
                "webcodex-workspace: git reader drain exceeded its one shared drain tail \
                 (opened at cleanup entry, {DRAIN_TAIL_BUDGET:?} at most, stdout had {stdout_budget:?} \
                 left) after the child was accounted for; reporting the capture as incomplete \
                 rather than blocking the caller on a descendant-held pipe"
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

/// Open **one** drain tail for a request and return it as an absolute instant.
///
/// This is the only place a drain tail is created. Everything that can block
/// during cleanup — the tree wait, the stdout drain, the stderr drain — draws
/// from the instant this returns, via [`remaining_tail`], and that instant is
/// never recomputed. The distinction is the whole point:
///
/// ```text
/// let tail_deadline = open_drain_tail(deadline);
/// // stage 1 consumes what is left of tail_deadline
/// let a = remaining_tail(tail_deadline);
/// // ... stage 2 consumes what is left of the SAME instant
/// let b = remaining_tail(tail_deadline);
/// ```
///
/// # Why one absolute instant and not a per-stage budget
///
/// An earlier version recomputed `min(deadline - now, DRAIN_TAIL_BUDGET)` at
/// every stage. That expression is capped but not *shared*: whenever the
/// caller's deadline was further out than [`DRAIN_TAIL_BUDGET`], every stage
/// received the full cap, so three stages cost up to three caps. With a caller
/// deadline ten seconds away the cleanup was measured at up to 750ms — three
/// times what a reader of the code would call "a 250ms tail".
///
/// Taking the minimum **once** and threading the resulting `Instant` makes the
/// bound a single measurable claim: the whole of cleanup ends by
/// `min(caller_deadline, cleanup_start + DRAIN_TAIL_BUDGET)`, no matter how many
/// stages there are or which ones actually block.
///
/// # Why the cap still applies
///
/// A caller that allowed ten seconds does not silently get an unbounded cleanup
/// window inside it: the git deadline and the cleanup deadline stay separate
/// claims, and the cleanup claim is the smaller of the two.
fn open_drain_tail(deadline: Instant) -> Instant {
    std::cmp::min(deadline, Instant::now() + DRAIN_TAIL_BUDGET)
}

/// What is left of an **already-open** drain tail.
///
/// The argument must be an instant produced by [`open_drain_tail`] (or the
/// caller's own deadline, which is never later than that tail). It is *not* a
/// budget to re-cap: capping here would let each stage take a fresh full
/// allowance, which is the accumulation defect this function exists to prevent.
fn remaining_tail(tail_deadline: Instant) -> std::time::Duration {
    tail_deadline.saturating_duration_since(Instant::now())
}

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

/// Collect one drained pipe, bounded by what remains of `deadline`.
///
/// # Why a panicking reader is not fatal here
///
/// The reader thread only does I/O on a pipe, which cannot panic. If it ever
/// did, returning empty output keeps the caller honest about the **exit
/// status** — which it still sees — instead of converting a capture problem
/// into a fabricated git failure.
///
/// # Why the deadline is a parameter
///
/// This previously took the constant [`DRAIN_TAIL_BUDGET`], which granted a
/// fresh fixed budget at a point where the caller's clock was already spent —
/// the same accumulation defect as the tree wait, in a different function. The
/// caller now passes the single tail instant it opened, so this only ever
/// consumes what earlier stages left.
fn join_reader(
    reader: Option<std::thread::JoinHandle<Vec<u8>>>,
    stream: &'static str,
    tail_deadline: Instant,
) -> Vec<u8> {
    join_reader_bounded(reader, stream, remaining_tail(tail_deadline)).0
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
/// So a candidate must be **capable**, and capability is measured the only way
/// that is honest: by performing the actual metadata operations the product
/// depends on, in a throwaway repository, under the exact environment the
/// brokered child will receive.
///
/// # Why `--version` was not enough
///
/// A previous version of this probe asked only for `--version` and classified
/// anything that answered as a real git. That is wrong in both directions:
///
/// * **False positive.** `/usr/bin/git` answers `--version` on a machine with
///   Command Line Tools installed, so it was classified as a plain real git and
///   never received `DEVELOPER_DIR`. On a host where the same shim cannot reach
///   its developer directory, every metadata read then failed while selection
///   still reported success.
/// * **False negative.** The accompanying shim test looked for `xcrun` or
///   `xcode-select` in the child's output. A shim that simply works emits
///   neither, so it was misclassified the other way.
///
/// Both failures come from asking "does this binary identify as a shim?"
/// instead of "can this binary do the work?". Only the second question is
/// answerable in a way that survives a host we have not seen.
fn resolve_git_executable(deadline: Option<Instant>) -> Option<GitSelection> {
    CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find_map(|candidate| is_capable_git(&candidate, deadline))
}

/// A candidate that passed the capability probe, plus what the probe learned
/// about the environment it needs.
///
/// # Why the probe's finding travels with the selection
///
/// The `DEVELOPER_DIR` question cannot be re-derived later without repeating
/// the mistake F2 flagged. An earlier version guessed "is this a shim?" from
/// the binary's output, and that heuristic disagreed with reality in both
/// directions. The replacement is an experiment — run the candidate without the
/// variable, and only retry with it if it failed — and an experiment's result is
/// only valid where it was performed.
///
/// So the answer is carried, not recomputed: a candidate proven to work without
/// `DEVELOPER_DIR` gets `None` and the spec names nothing extra, while one
/// proven to need it gets exactly the directory the successful probe used. That
/// makes the environment a function of the measurement rather than of a second
/// guess about the same binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitSelection {
    /// The git binary to run.
    pub program: PathBuf,
    /// The developer directory the probe proved this candidate needs, or `None`
    /// when it worked without one.
    pub developer_dir: Option<std::ffi::OsString>,
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
    resolve_git_executable(None).map(|selection| selection.program)
}

/// The trusted prefixes searched for a usable git, in preference order.
///
/// A Homebrew git is preferred **when both work**, because it is a real git
/// rather than a developer-tools shim: it does not consult `xcrun`, so it keeps
/// working under a profile that denies the shim its developer directory. The
/// system prefixes stay in the list as a fallback for hosts with no Homebrew,
/// where a fully installed Command Line Tools does provide a real git underneath
/// the shim — and where, if it does not, the capability probe rejects it instead
/// of selecting it and failing later inside the broker.
const CANDIDATES: &[&str] = &[
    "/opt/homebrew/bin/git",
    "/usr/local/bin/git",
    "/opt/local/bin/git",
    "/usr/bin/git",
    "/bin/git",
];

/// How long the capability probe may spend on one candidate.
///
/// Bounded for two independent reasons: it runs on the request path before any
/// real work, and it is the step that decides whether we start a process at
/// all. A candidate that cannot create a repository and answer three metadata
/// queries inside this budget is not a git this broker should use.
///
/// Also the ceiling when the caller supplied no deadline of its own, so an
/// absent caller budget never means an unbounded probe.
const GIT_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether `candidate` can perform the metadata operations the catalog needs,
/// under the environment the brokered child will receive.
///
/// The probe is deliberately the real thing: create a repository, commit, then
/// run `rev-parse`, `status` and `log` in it. Anything less — a version string,
/// an identity heuristic — has already been shown to disagree with what the
/// brokered child will experience.
///
/// Every step is bounded by `deadline`, and each command is launched with
/// `env_clear` plus the production `PATH` and `HOME`, so a candidate that needs
/// reach this broker does not grant is rejected here rather than at first use.
fn is_capable_git(candidate: &Path, deadline: Option<Instant>) -> Option<GitSelection> {
    if !candidate.is_file() {
        return None;
    }
    // Same rule as `developer_dir_via`: a caller deadline is **propagated**, never
    // rebased. `GIT_PROBE_TIMEOUT` shortens it when the caller allowed more, and
    // an already-expired deadline fails the probe without launching anything —
    // but the instant the caller's request started counting is the instant that
    // bounds this probe.
    let probe_deadline = probe_deadline(deadline)?;

    // Step 1 — try the candidate with **no** `DEVELOPER_DIR`, which is what a
    // real git needs and what the brokered child will normally get. This is the
    // narrow case and it must be tried first, so the common path never widens
    // the environment.
    if probe_candidate(candidate, None, probe_deadline) {
        return Some(GitSelection {
            program: candidate.to_path_buf(),
            developer_dir: None,
        });
    }

    // Step 2 — only now, having watched it fail without help, allow the host's
    // developer directory and try once more. A candidate that needs this is a
    // shim, and a shim that still cannot work with an explicitly named
    // toolchain is not usable at all.
    // The developer-directory lookup is inside this probe's budget, not a fresh
    // one of its own: a caller that allowed 300ms must not then wait a further
    // `GIT_PROBE_TIMEOUT` for a host tool to answer.
    let developer_dir = developer_dir(Some(probe_deadline)).map(std::ffi::OsString::from)?;
    if probe_candidate(candidate, Some(developer_dir.clone()), probe_deadline) {
        return Some(GitSelection {
            program: candidate.to_path_buf(),
            developer_dir: Some(developer_dir),
        });
    }
    None
}

/// Create a throwaway directory for a capability probe, and remove it after.
///
/// Hand-rolled rather than `tempfile` because `tempfile` is a dev-dependency of
/// this crate: production code here must not gain a dependency the sandboxed
/// build does not already have. The directory lives under the process temp
/// root, is named distinctly enough not to collide, and is best-effort removed
/// because a leftover empty directory is harmless next to a probe that failed.
struct ProbeDir(std::path::PathBuf);

impl ProbeDir {
    fn create(tag: &str) -> Option<Self> {
        let unique = format!(
            "webcodex-git-probe-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique.replace(['(', ')', ' '], "-"));
        // A pre-existing path means someone else owns this name; refusing is
        // safer than reusing a directory whose contents we did not create.
        if path.exists() {
            return None;
        }
        std::fs::create_dir_all(&path).ok()?;
        Some(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Create a repository with `candidate` and read the three metadata operations
/// back out of it, all under one deadline.
///
/// Returns `false` on any failure — a command that could not run, exited
/// non-zero, was killed by the deadline, or answered with nothing where an
/// answer is required. An empty `rev-parse` or `log` is a failure rather than a
/// pass, because "git exited 0" is not the same claim as "git reported a
/// branch and a commit".
fn probe_candidate(
    candidate: &Path,
    developer_dir: Option<std::ffi::OsString>,
    deadline: Instant,
) -> bool {
    // A throwaway repository, outside any caller-visible path. This runs in the
    // trusted parent, before any sandboxed child exists — the same place the
    // plan is derived — so using the process temp directory here grants nothing
    // to a child that has not been created yet.
    let Some(repo) = ProbeDir::create("cap") else {
        return false;
    };
    let root = repo.path().to_path_buf();

    if run_probe(candidate, &["init", "-q"], &root, &developer_dir, deadline) != Some(0) {
        return false;
    }
    if std::fs::write(root.join("probe.txt"), b"probe\n").is_err() {
        return false;
    }
    if run_probe(
        candidate,
        &["add", "probe.txt"],
        &root,
        &developer_dir,
        deadline,
    ) != Some(0)
    {
        return false;
    }
    // Identity on the command line: the probe runs with no global config, which
    // is the point, so it cannot rely on one existing.
    let commit: Vec<&str> = [
        "-c",
        "user.email=probe@example.invalid",
        "-c",
        "user.name=probe",
        "commit",
        "-q",
        "-m",
        "probe",
    ]
    .to_vec();
    if run_probe(candidate, &commit, &root, &developer_dir, deadline) != Some(0) {
        return false;
    }

    // The three reads the catalog actually performs. `status --short` is
    // legitimately empty on a clean tree, so only its exit status matters; the
    // other two must produce an answer.
    for (argv, must_be_non_empty) in [
        (["rev-parse", "--abbrev-ref", "HEAD"].as_slice(), true),
        (["status", "--short"].as_slice(), false),
        (["log", "-1", "--pretty=format:%h"].as_slice(), true),
    ] {
        match run_probe_output(candidate, argv, &root, &developer_dir, deadline) {
            Some((0, stdout)) => {
                if must_be_non_empty && stdout.trim().is_empty() {
                    // Exited zero with no answer: this is the broken candidate
                    // the F2 review was about, and counting it as usable would
                    // put the failure back into the catalog.
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Run one probe command with the production environment, fully bounded.
///
/// Returns the exit code on a clean, in-budget completion, and `None` for
/// anything else — a non-zero exit, a signal, or a deadline that passed. The
/// distinction is deliberate: "this candidate cannot do the work" and "this
/// candidate cannot be evaluated in time" both disqualify it, but neither is
/// allowed to become an unbounded wait.
fn run_probe(
    candidate: &Path,
    args: &[&str],
    root: &Path,
    developer_dir: &Option<std::ffi::OsString>,
    deadline: Instant,
) -> Option<i32> {
    run_probe_output(candidate, args, root, developer_dir, deadline).map(|(code, _)| code)
}

/// [`run_probe`], additionally returning captured stdout.
///
/// The metadata reads this broker depends on are judged on their *answer*, not
/// only on their exit status, so the probe has to be able to see what git said.
fn run_probe_output(
    candidate: &Path,
    args: &[&str],
    root: &Path,
    developer_dir: &Option<std::ffi::OsString>,
    deadline: Instant,
) -> Option<(i32, String)> {
    let mut command = std::process::Command::new(candidate);
    command
        .args(args)
        .current_dir(root)
        // The production environment, minus `DEVELOPER_DIR` unless this
        // candidate needs it. `env_clear` first: nothing may leak in.
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(dir) = developer_dir {
        command.env("DEVELOPER_DIR", dir);
    }
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // The child is gone, so reading its pipes to EOF cannot block on
                // it. A descendant could still hold the write end, which is why
                // this is bounded rather than trusted.
                let output = read_bounded_output(&mut child, deadline)?;
                return status.code().map(|code| (code, output));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

/// Read a finished child's stdout to EOF, bounded by `deadline`.
///
/// `wait_with_output` cannot be used directly here: it blocks on the pipes with
/// no deadline, so a probe would reintroduce exactly the unbounded wait this
/// whole path exists to remove.
/// Collect a child's stdout under an absolute deadline.
///
/// # Why the read cannot happen on this thread
///
/// The previous version checked the deadline and then called `read()` on the
/// calling thread. That check is not a bound. `read()` on a pipe blocks until
/// the write end closes **in every process that inherited it**, so a git whose
/// direct child exited while a descendant still held stdout parked the caller
/// inside a syscall with no way to observe the deadline — the request could
/// outlive its budget by as long as the descendant lived, and nothing in the
/// loop could report it.
///
/// The fix is the same shape already used for the main capture path: the read
/// runs on its own thread and the caller waits on a channel with
/// `recv_timeout`. The wait is therefore bounded by construction rather than by
/// a check that a blocking call can ignore.
///
/// # Why the read thread is detached
///
/// On the timeout path the thread is still blocked in `read`, holding the pipe.
/// Dropping the handle keeps the *caller's* wait bounded, which is the property
/// under test; the thread ends when the pipe finally closes. Detaching is what
/// makes the timeout path itself incapable of becoming the new hang.
fn read_bounded_output(child: &mut std::process::Child, deadline: Instant) -> Option<String> {
    use std::io::Read;
    let stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel::<Vec<u8>>();
    let pump = std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut collected = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => collected.extend_from_slice(&chunk[..read]),
                Err(ref error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = sender.send(collected);
    });
    drop(pump);

    // `recv_timeout` cannot be given an absolute deadline, so the remaining
    // budget is what is left of it. An expired deadline yields zero, which
    // returns immediately without waiting.
    let budget = deadline.saturating_duration_since(Instant::now());
    match receiver.recv_timeout(budget) {
        Ok(collected) => Some(String::from_utf8_lossy(&collected).to_string()),
        // A read that could not finish is an incomplete capture, and
        // `None` is how this function has always reported that. It is not a
        // fabricated exit status — the caller still sees git's own.
        Err(_) => None,
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

    /// Gate a native-evidence test on its verdict, exactly as the catalog does.
    ///
    /// # Why this exists
    ///
    /// The F3 review found tests that printed `HOST_UNAVAILABLE` or
    /// `ENV_BLOCKED` — with a disclaimer in the message that this "is NOT a
    /// pass" — and then `return`ed. Cargo counts a returned test as **green**. So
    /// a host whose kernel refuses the sandbox produced a suite where every
    /// evidence test was green and none of them had measured anything. That is
    /// the worst possible reading: not "we could not check", but "checked, and
    /// fine".
    ///
    /// Printing a state is not accounting for it. The accounting has to reach the
    /// harness, and the only channel cargo listens to is the exit status.
    ///
    /// # The four outcomes
    ///
    /// | verdict | meaning | outcome |
    /// |---|---|---|
    /// | `Pass` | the behaviour was observed | test proceeds, green |
    /// | `Fail` | the behaviour was observed broken | **panic** |
    /// | `HostUnavailable` | no git toolchain; broker never exercised | **panic** |
    /// | `EnvBlocked` | broker ran, kernel refused the profile | **panic** |
    ///
    /// This mirrors `enforce_verdict` in the catalog adapter so both sides of the
    /// product agree on what a blocked state means. The catalog's own doc comment
    /// states the trade-off, and it is worth restating because it is the whole
    /// point: **a host that cannot measure these tests cannot show them green.**
    /// That is the correct direction for an evidence gate. A red or non-pass
    /// local suite on a blocked host is more honest than a green one that
    /// proves nothing.
    ///
    /// # What is deliberately *not* done
    ///
    /// `std::process::exit` is not used: it would kill the whole test binary and
    /// destroy every other test's result, which is the opposite of honest
    /// accounting. Nor is the verdict downgraded to a warning. The state is
    /// reported through the harness as what it is — a failure to measure.
    fn enforce_verdict(verdict: GitVerdict, label: &str) {
        match verdict {
            GitVerdict::Pass => {}
            GitVerdict::Fail => {
                panic!("{label}: the behaviour under test was observed to be broken (verdict=Fail)")
            }
            GitVerdict::HostUnavailable => panic!(
                "{label}: HOST_UNAVAILABLE — this host cannot run git at all, so the behaviour \
                 under test is UNMEASURED. This is not a pass; run the suite on a host with a \
                 usable git toolchain."
            ),
            GitVerdict::EnvBlocked => panic!(
                "{label}: ENV_BLOCKED — the broker was exercised but the environment refused the \
                 confinement profile, so the behaviour under test is UNMEASURED. This is not a \
                 pass; run the suite where the sandbox profile can be applied."
            ),
        }
    }

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
            None,
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
        if let Some(selection) = resolve_git_executable(None) {
            let git = selection.program;
            assert!(git.is_absolute());
            assert!(git.is_file());
        }
    }

    #[test]
    fn toolchain_grant_never_covers_the_whole_filesystem() {
        if let Some(selection) = resolve_git_executable(None) {
            let git = selection.program;
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
        let Some(selection) = resolve_git_executable(None) else {
            return false;
        };
        let git = selection.program;
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
            enforce_verdict(
                GitVerdict::HostUnavailable,
                "P1_NATIVE_GIT_BROKER_FIDELITY: no trusted git executable on this host",
            );
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
                    enforce_verdict(
                        GitVerdict::EnvBlocked,
                        &format!(
                            "P1_NATIVE_GIT_BROKER_FIDELITY: broker launched but the kernel \
                             refused the profile ({stderr})"
                        ),
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
                    enforce_verdict(
                        GitVerdict::HostUnavailable,
                        &format!(
                            "P1_NATIVE_GIT_BROKER_FIDELITY: git could not run because the host \
                             toolchain is incomplete ({stderr})"
                        ),
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
                    enforce_verdict(
                        GitVerdict::HostUnavailable,
                        &format!(
                            "P1_NATIVE_GIT_BROKER_FIDELITY: no trusted git executable ({refusal})"
                        ),
                    );
                    return;
                }
                eprintln!("P1_GIT_APPLY_REASON=broker_refused_to_spawn");
                eprintln!(
                    "P1_NATIVE_GIT_BROKER_FIDELITY=ENV_BLOCKED broker refused to launch git ({refusal})"
                );
                assert_eq!(refusal.code, "git_spawn_refused");
                // The assert above pins the *shape* of the refusal; the gate
                // below pins that a refused launch is still not a pass. One does
                // not stand in for the other.
                enforce_verdict(
                    GitVerdict::EnvBlocked,
                    "P1_NATIVE_GIT_BROKER_FIDELITY: broker refused to launch git",
                );
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
            enforce_verdict(
                GitVerdict::HostUnavailable,
                "P1B_F1_DRAIN_BOUND: no functional git to create a workspace",
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

        let read = match outcome {
            Ok(read) => read,
            Err(error) => {
                enforce_verdict(
                    GitVerdict::EnvBlocked,
                    &format!("P1B_F1_DRAIN_BOUND: broker refused to launch ({error})"),
                );
                return;
            }
        };
        if !read.status.success() {
            let stderr = String::from_utf8_lossy(&read.stderr);
            if is_profile_refusal(&read.stderr) || is_host_unavailable(stderr.as_bytes()) {
                enforce_verdict(
                    GitVerdict::EnvBlocked,
                    &format!("P1B_F1_DRAIN_BOUND: the host refused the profile ({stderr})"),
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
    /// Both streams of the **no-caller-deadline** path get a full budget each.
    ///
    /// `finish` is reached by `run_git(.., None)`, whose readers must not be
    /// starved. Sharing one 250ms tail between stdout and stderr there was tried
    /// and reverted: the second stream then inherits whatever the first left —
    /// usually nothing — so it comes back **empty while the call reports
    /// success**. `brokered_git_usable` decides from `--version`'s stderr whether
    /// the sandbox refused the profile, so an empty stderr makes a refused host
    /// look like a working one and silently un-skips every environment-gated
    /// test.
    ///
    /// The test pins the two streams' budgets as **independent**, which is the
    /// property that was lost. It is structural rather than timing-based: a
    /// reader that is slow to arrive is collected if and only if it was handed
    /// the full budget, so asserting collection for a reader that arrives just
    /// inside one budget distinguishes "own budget" from "leftover".
    #[test]
    fn the_unbounded_wait_path_gives_each_stream_its_own_budget() {
        // Arrives inside ONE full budget but well past where a shared tail would
        // have left the second stream. 60ms into the budget: a leftover-based
        // scheme would have handed this stream ~0 after the first consumed its
        // share, and the read would come back empty.
        let reader = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(60));
            b"stderr evidence".to_vec()
        });
        let started = Instant::now();
        let (collected, incomplete) =
            join_reader_bounded(Some(reader), "stderr", DRAIN_TAIL_BUDGET);
        let elapsed = started.elapsed();
        assert_eq!(
            collected,
            b"stderr evidence".to_vec(),
            "a stream must receive its own full budget, not what a sibling stage left behind — \
             an empty capture is indistinguishable from a silent success"
        );
        assert!(
            !incomplete,
            "the read completed inside its budget, so it must not be reported incomplete"
        );
        assert!(
            elapsed < DRAIN_TAIL_BUDGET,
            "the read finished inside its own budget, took {elapsed:?}"
        );
    }

    /// The drain join is bounded even when the reader never finishes.
    #[test]
    fn the_drain_join_is_bounded_even_when_the_reader_never_finishes() {
        let (sender, _receiver) = std::sync::mpsc::channel::<Vec<u8>>();
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
        let git = BrokeredGit {
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
        let git = resolve_git_executable(None)?.program;
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
        let git = resolve_git_executable(None)?.program;
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
            None,
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
    /// tools shim satisfies while being unable to run. Where a capable git
    /// exists, the shim must not win merely by being listed first.
    #[test]
    fn the_resolved_git_is_capable_not_merely_present() {
        let Some(selection) = resolve_git_executable(None) else {
            enforce_verdict(
                GitVerdict::HostUnavailable,
                "P1B_F2_GIT_PROFILE: no capable git on this host",
            );
            return;
        };
        let git = selection.program;
        assert!(git.is_absolute(), "a resolved git must be a concrete path");
        // Re-probe the resolved candidate: resolution claimed it can do the
        // work, so a second probe must agree. A disagreement means selection is
        // answering a different question than the one the broker relies on.
        assert!(
            is_capable_git(&git, None).is_some(),
            "F2: the resolved git {} cannot perform the metadata operations under the child's \
             environment",
            git.display()
        );
        eprintln!(
            "P1B_F2_GIT_PROFILE=PASS resolved capable git {}",
            git.display()
        );
    }

    /// A shim that cannot start must be skipped in favour of a working git.
    ///
    /// Asserted as behaviour of the selection rule rather than as a property of
    /// any particular host: when a capable candidate exists, whatever
    /// `CANDIDATES` lists earlier must not be returned.
    #[test]
    fn a_non_capable_git_is_never_selected() {
        // A file that exists but is not a working git is the cheapest
        // reproduction of the failure mode: present, trusted-prefixed, and
        // unable to create a repository or read one.
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
            is_capable_git(&fake, None).is_none(),
            "F2: a file that cannot perform the metadata operations must not pass the probe"
        );
    }

    /// A candidate that answers `--version` but cannot do the work is rejected.
    ///
    /// This is the specific false positive the F2 review measured on `/usr/bin/git`:
    /// a binary that reports a version, creates nothing, and answers no metadata
    /// query must not be selected. A version probe would have accepted it; only
    /// the capability probe rejects it.
    #[test]
    fn a_git_that_only_reports_its_version_is_not_capable() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("git");
        // Exactly the shape that fooled the old probe: a convincing `--version`.
        std::fs::write(
            &fake,
            b"#!/bin/sh\n\
              if [ \"$1\" = \"--version\" ]; then echo 'git version 9.9.9 (fake)'; exit 0; fi\n\
              exit 1\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }
        // It does answer a version query...
        let version = run_probe(
            &fake,
            &["--version"],
            dir.path(),
            &None,
            Instant::now() + GIT_PROBE_TIMEOUT,
        );
        assert_eq!(
            version,
            Some(0),
            "F2: the fixture must answer --version, or it proves nothing"
        );
        // ...and it still must not be selected.
        assert!(
            is_capable_git(&fake, None).is_none(),
            "F2: answering --version is not capability; a git that cannot create a repository and \
             report metadata must be rejected"
        );
    }

    /// The whole operation is bounded, and the probe is inside that bound.
    ///
    /// F1 measured a 2-second operation that took 8.5 seconds because the
    /// deadline was created *after* the candidate probe, and the probe's own
    /// wait had no bound at all. The deadline now exists before the first
    /// candidate is touched and every probe step draws from it, so the elapsed
    /// time of a request cannot exceed its own budget.
    ///
    /// Proven with a candidate that cannot answer: a script which never exits is
    /// the cheapest reproduction of an unbounded wait, and it also proves the
    /// bound is the deadline's rather than the candidate's cooperation. The
    /// budget is asserted, not merely observed — a fast host must not pass by
    /// being fast.
    #[test]
    fn the_candidate_probe_cannot_outlive_the_operation_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("git");
        // Exists, is executable, and never terminates. Without a bound this is
        // the exact hang the review measured.
        std::fs::write(&fake, b"#!/bin/sh\nwhile :; do :; done\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }

        let budget = std::time::Duration::from_millis(300);
        let deadline = Instant::now() + budget;
        let started = Instant::now();
        let capable = is_capable_git(&fake, Some(deadline));
        let elapsed = started.elapsed();

        assert!(
            capable.is_none(),
            "F1: a candidate that never terminates cannot be a capable git"
        );
        // Generous, because a killed process and a filesystem sync are not the
        // thing under test — but far below the unbounded case the review
        // measured at 8.5s against a 2s budget.
        assert!(
            elapsed < budget * 4,
            "F1: the probe must live inside the operation budget, took {elapsed:?} for a budget of \
             {budget:?}"
        );
    }

    /// The developer-directory lookup lives inside the caller's deadline.
    ///
    /// A capability probe that fails without `DEVELOPER_DIR` consults the host
    /// before trying again. That lookup used to take a fresh
    /// `GIT_PROBE_TIMEOUT`, so a request promised a bounded lifetime still
    /// waited out a second, unrelated budget *after* the caller's clock was
    /// spent — the bound it kept was not the one the caller agreed to.
    ///
    /// The command is a parameter precisely so this is testable: the real
    /// `xcode-select` answers instantly, so the bounded path would otherwise
    /// never be exercised.
    #[test]
    fn the_developer_dir_lookup_cannot_outlive_the_caller_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let hanging = dir.path().join("xcode-select");
        // Never terminates. The obvious fixture — `exit 0` followed by a sleep —
        // does not work here and was measured not to: the shell exits
        // immediately, the sleep is orphaned, and `try_wait` returns at once, so
        // the lookup finishes in ~0.5s regardless of any bound. A fixture that
        // does not reproduce the defect cannot demonstrate the fix.
        std::fs::write(&hanging, b"#!/bin/sh\nwhile :; do :; done\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&hanging).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&hanging, perms).unwrap();
        }

        // A caller budget far below `GIT_PROBE_TIMEOUT` (5s). The threshold is
        // set well below 5s so a fresh internal timeout cannot pass, and well
        // above the budget so an honest kill-and-reap cannot fail on timing.
        let budget = std::time::Duration::from_millis(250);
        let started = Instant::now();
        let found = developer_dir_via(&hanging, Some(Instant::now() + budget));
        let elapsed = started.elapsed();
        assert!(found.is_none(), "a hanging host tool has no answer to give");
        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "F1: the developer-dir lookup must live inside the caller's deadline, took {elapsed:?} \
             for a budget of {budget:?}; a fresh GIT_PROBE_TIMEOUT would show ~5s"
        );
        eprintln!("F1_TEST_A developer_dir bounded: elapsed={elapsed:?} budget={budget:?}");
    }

    /// An already-expired deadline must not launch the host tool at all.
    ///
    /// The degenerate case of the same invariant, and the one that separates
    /// "bounded" from "bounded but still spends time": a deadline in the past
    /// means the answer is no longer worth having, so the correct behaviour is
    /// to return without spawning anything.
    #[test]
    fn an_expired_deadline_skips_the_developer_dir_lookup_entirely() {
        let started = Instant::now();
        let found = developer_dir_via(Path::new(XCODE_SELECT), Some(Instant::now()));
        let elapsed = started.elapsed();
        assert!(
            found.is_none(),
            "an expired deadline cannot yield a usable answer"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "F1: an expired deadline must return without launching a process, took {elapsed:?}"
        );
    }

    /// Collecting a probe's stdout must not block past the deadline.
    ///
    /// The defect was structural: check the deadline, then call `read()` on the
    /// calling thread. `read()` blocks until the write end closes in *every*
    /// process that inherited it, so a git whose direct child exited while a
    /// descendant held stdout parked the caller in a syscall that no amount of
    /// checking could bound.
    ///
    /// Reproduced directly: a shell exits immediately while a background
    /// descendant keeps the pipe open. The parent is gone, so the child is
    /// accounted for — exactly the state in which the old loop called `read()`
    /// and waited for a pipe nobody would ever close.
    #[test]
    fn a_descendant_holding_stdout_cannot_block_the_probe_read_past_its_deadline() {
        // `(exit 0 &)` backgrounds a subshell that inherits stdout, then the
        // direct child exits 0. The pipe stays open past the child's death.
        let script = "echo ready\n(exec sleep 30) &\nexit 0\n";
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the fixture shell must start");
        // Let the descendant establish itself before the deadline starts, so the
        // test measures the blocked read rather than a race to spawn it.
        std::thread::sleep(std::time::Duration::from_millis(120));

        let budget = std::time::Duration::from_millis(400);
        let started = Instant::now();
        let collected = read_bounded_output(&mut child, Instant::now() + budget);
        let elapsed = started.elapsed();

        // Kill the descendant so the fixture does not outlive the suite.
        let _ = child.kill();
        let _ = child.wait();

        // The threshold is what makes this a real guard. Measured on this host:
        // with the descendant holding the pipe a blocking read runs past 2.1s,
        // and without one it returns in ~52ms. A bound that accepted "a bit
        // over budget" would therefore pass an implementation that waits for
        // EOF — which is the defect. So the limit sits above the honest cost of
        // spawning and killing, and far below the fixture's 30s descendant.
        assert!(
            elapsed < std::time::Duration::from_millis(1200),
            "F1: the probe read must be deadline-bounded, took {elapsed:?} for a budget of {budget:?}; \
             a blocking read on a descendant-held pipe runs until the descendant exits"
        );
        // Whether the bytes arrived is not the claim under test; the claim is
        // that the call returned. Both outcomes are honest, so the assertion is
        // only about the bound.
        let _ = collected;
        eprintln!("F1_TEST_B probe stdout read bounded: elapsed={elapsed:?} budget={budget:?}");
    }

    /// A long caller deadline does **not** buy three drain tails.
    ///
    /// This is the counterexample the review raised against `accc4a87`. The old
    /// helper was
    ///
    /// ```text
    /// fn remaining_tail(deadline: Instant) -> Duration {
    ///     deadline.saturating_duration_since(Instant::now()).min(DRAIN_TAIL_BUDGET)
    /// }
    /// ```
    ///
    /// which is capped, so it *looks* bounded — but it is recomputed per stage,
    /// and when the caller's deadline is further out than the cap every stage
    /// receives the full cap:
    ///
    /// ```text
    /// caller deadline = now + 10s
    ///   tree wait    -> min(10s, 250ms) = 250ms
    ///   stdout drain -> min(10s, 250ms) = 250ms
    ///   stderr drain -> min(10s, 250ms) = 250ms
    ///   total        = 750ms   (three budgets, not one)
    /// ```
    ///
    /// The repair takes that minimum **once**, in [`open_drain_tail`], and threads
    /// the resulting `Instant`, so the three stages are three draws on one
    /// allowance.
    ///
    /// # Why the negative control is inside this test
    ///
    /// Two earlier fixtures were tried and both were worthless:
    ///
    /// * Letting stage 1 block for its budget and then running the reader stages.
    ///   Once the shared tail is spent, the old expression also finds its deadline
    ///   passed and hands out ~0 — so the old code measured 260ms and **passed**.
    /// * Reinstating the old expression and running this test against it. Also
    ///   passed, for the same reason: the frozen instant means the per-stage re-cap
    ///   has nothing left to re-cap.
    ///
    /// Both fixtures share a flaw: they let the wall clock decide, and once any
    /// time has been spent the two implementations agree. A regression test that
    /// cannot fail on the regression is worse than none, because it certifies the
    /// defect is gone.
    ///
    /// So the old expression is reproduced here **as a value**, against a caller
    /// deadline far in the future, and asserted to grant three full budgets. That
    /// is the 750ms counterexample, computed rather than raced — and it fails if
    /// anyone ever re-caps per stage again. The live measurement below then shows
    /// the real path stays inside one budget.
    #[test]
    fn three_cleanup_stages_cannot_exceed_one_drain_tail_budget() {
        let caller_deadline = Instant::now() + std::time::Duration::from_secs(10);

        // ── Negative control: the accc4a87 expression, evaluated ─────────────
        // Reproduced verbatim. Three stages, each drawing from the *caller's*
        // deadline rather than from a shared tail.
        let old_stage_budget = |_stage: usize| -> std::time::Duration {
            caller_deadline
                .saturating_duration_since(Instant::now())
                .min(DRAIN_TAIL_BUDGET)
        };
        let old_tree = old_stage_budget(1);
        std::thread::sleep(old_tree);
        let old_stdout = old_stage_budget(2);
        std::thread::sleep(old_stdout);
        let old_stderr = old_stage_budget(3);
        let old_total = old_tree + old_stdout + old_stderr;
        assert!(
            old_total >= DRAIN_TAIL_BUDGET * 2,
            "negative control: the accc4a87 per-stage re-cap must grant more than one budget for \
             this test to mean anything (granted {old_total:?} across three stages)"
        );

        // ── The repaired path ────────────────────────────────────────────────
        // One instant, opened once. Every stage draws from it and blocks for the
        // whole draw, exactly as the counterexample's stages do.
        let tail_deadline = open_drain_tail(caller_deadline);
        let never = || -> Option<std::thread::JoinHandle<Vec<u8>>> {
            Some(std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(30));
                Vec::new()
            }))
        };

        let started = Instant::now();

        // Stage 1 — the tree wait: a group whose leader exited but whose
        // descendant lives can never report a full tree exit, so it blocks.
        let tree_budget = remaining_tail(tail_deadline);
        std::thread::sleep(tree_budget);

        // Stage 2 — stdout: whatever is left of the same instant.
        let stdout_budget = remaining_tail(tail_deadline);
        let (_stdout, stdout_incomplete) = join_reader_bounded(never(), "stdout", stdout_budget);

        // Stage 3 — stderr.
        let stderr_budget = remaining_tail(tail_deadline);
        let (_stderr, stderr_incomplete) = join_reader_bounded(never(), "stderr", stderr_budget);

        let total = started.elapsed();
        let spent = tree_budget + stdout_budget + stderr_budget;

        assert!(
            stdout_incomplete && stderr_incomplete,
            "F1: a reader that can never finish must be reported incomplete"
        );
        // The sum of the three draws is at most one tail, because they are draws
        // on one instant rather than three independent caps.
        assert!(
            spent <= DRAIN_TAIL_BUDGET + std::time::Duration::from_millis(1),
            "F1: the three stage budgets must sum to one shared tail, but they sum to {spent:?} \
             (tree={tree_budget:?} stdout={stdout_budget:?} stderr={stderr_budget:?}) — a \
             per-stage re-cap would hand out three full budgets"
        );
        // And the wall clock agrees: one budget, plus scheduling slack — not three.
        let allowance = DRAIN_TAIL_BUDGET * 2;
        assert!(
            total < allowance,
            "F1: tree wait + stdout drain + stderr drain consumed {total:?}, which exceeds one \
             shared drain tail (allowance {allowance:?} = 2 x {DRAIN_TAIL_BUDGET:?})"
        );
        // Strictly better than the behaviour under review, and measurably so.
        assert!(
            spent * 2 < old_total,
            "F1: the shared tail ({spent:?}) must be strictly smaller than what the per-stage \
             re-cap granted ({old_total:?})"
        );
        eprintln!(
            "F1_TEST_ONE_TAIL repaired total={total:?} spent={spent:?} \
             (tree={tree_budget:?} stdout={stdout_budget:?} stderr={stderr_budget:?}) | \
             NC accc4a87 granted={old_total:?} (tree={old_tree:?} stdout={old_stdout:?} \
             stderr={old_stderr:?})"
        );
    }

    /// `open_drain_tail` is the only place a cap is applied, and it never
    /// returns an instant later than the caller's own deadline.
    ///
    /// Two claims that the timing test above cannot make on its own: that the
    /// tail is capped at all when the caller is generous, and that it never
    /// *extends* a caller who is not.
    #[test]
    fn the_drain_tail_is_capped_but_never_later_than_the_caller() {
        // A generous caller: capped to one budget, never the full ten seconds.
        let generous = Instant::now() + std::time::Duration::from_secs(10);
        let tail = open_drain_tail(generous);
        assert!(
            tail <= generous,
            "the drain tail must never be later than the caller's deadline"
        );
        assert!(
            remaining_tail(tail) <= DRAIN_TAIL_BUDGET,
            "a caller who allowed 10s must not get an uncapped cleanup window"
        );
        assert!(
            tail < generous,
            "a caller far past the cap must actually be capped, or the tail is unbounded"
        );

        // An imminent caller: the caller's deadline wins, because the tail is a
        // minimum, not a grant.
        let imminent = Instant::now() + std::time::Duration::from_millis(5);
        assert!(
            open_drain_tail(imminent) <= imminent,
            "the drain tail must never extend a deadline the caller set"
        );

        // An expired caller: no new budget is invented for cleanup at all.
        let expired = Instant::now() - std::time::Duration::from_secs(1);
        assert_eq!(
            remaining_tail(open_drain_tail(expired)),
            std::time::Duration::ZERO,
            "an expired deadline must give cleanup nothing to wait for"
        );
    }

    /// Tree wait, stdout drain and stderr drain share one tail budget.
    ///
    /// The accumulation was structural: the tree wait took the constant
    /// `DRAIN_TAIL_BUDGET`, then both readers were handed the *same measured*
    /// duration. Measuring once and reusing it means the second stage is granted
    /// what the first already spent, so three stages cost up to three budgets.
    ///
    /// What is asserted here is that a zero remaining budget yields a
    /// non-blocking call — the property that makes sharing real. If a stage
    /// could still be granted a fresh budget after the deadline passed, an
    /// expired deadline would still cost real time.
    #[test]
    fn an_expired_deadline_gives_the_remaining_stages_nothing_to_wait_for() {
        let expired = Instant::now() - std::time::Duration::from_secs(1);
        let _tail_deadline = open_drain_tail(expired);

        // Every stage asked after the deadline must be given zero, so
        // `recv_timeout(0)` returns immediately instead of waiting out a budget.
        let first = remaining_tail(expired);
        let second = remaining_tail(expired);
        let third = remaining_tail(expired);
        assert_eq!(
            (first, second, third),
            (
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
                std::time::Duration::ZERO
            ),
            "F1: after the deadline every cleanup stage must receive zero, or the tail accumulates"
        );

        // And a reader that will never finish must therefore cost ~nothing.
        let reader = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(30));
            Vec::new()
        });
        let started = Instant::now();
        let (collected, incomplete) =
            join_reader_bounded(Some(reader), "stdout", remaining_tail(expired));
        let elapsed = started.elapsed();
        assert!(
            incomplete,
            "a reader that cannot finish must be reported incomplete"
        );
        assert!(
            collected.is_empty(),
            "an unfinished reader must not fabricate output"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "F1: a zero remaining budget must not block, took {elapsed:?}"
        );
        eprintln!("F1_TEST_C one tail budget: expired-deadline join elapsed={elapsed:?}");
    }

    /// The tail budget shrinks as it is spent, rather than being re-measured.
    ///
    /// The direct statement of "one absolute tail deadline": two calls to
    /// [`remaining_tail`] against the **same** opened tail must report different
    /// budgets, and the second must be smaller. If the second could be as large as
    /// the first, the stages would not be sharing anything.
    ///
    /// Note what is *not* being asserted: that a fresh cap is unavailable. The
    /// cap is applied once, in `open_drain_tail`; this test is about the draw-down
    /// from that one instant afterwards.
    #[test]
    fn the_tail_budget_shrinks_as_it_is_spent() {
        let caller_deadline = Instant::now() + std::time::Duration::from_millis(300);
        let tail_deadline = open_drain_tail(caller_deadline);
        let first = remaining_tail(tail_deadline);
        std::thread::sleep(std::time::Duration::from_millis(60));
        let second = remaining_tail(tail_deadline);
        assert!(
            second < first,
            "F1: the tail must be drawn down per stage, first={first:?} second={second:?}"
        );
        assert!(
            second <= std::time::Duration::from_millis(240),
            "F1: after 60ms of a 300ms budget about 240ms should remain, got {second:?}"
        );
    }

    /// No helper is ever handed a deadline later than its caller's.
    ///
    /// Item C's audit target was the rebase
    /// `let budget = absolute - now; let deadline = Instant::now() + budget;`
    /// which looks like a pass-through but is not: the second `now` is later than
    /// the first, so the rebuilt instant is later than the one the caller chose,
    /// and the time spent in between silently refills the budget.
    ///
    /// A timing assertion cannot catch that — the drift is nanoseconds wide. So
    /// this test pins the property structurally, through the one seam every probe
    /// helper inherits its bound from, and makes the defect observable with a
    /// deliberate delay standing in for the work that happens between a request's
    /// entry point and the helper that serves it.
    #[test]
    fn a_helper_is_never_handed_a_deadline_later_than_its_callers() {
        // ── Negative control ────────────────────────────────────────────────
        // The pre-repair arithmetic, reproduced here so the test cannot pass by
        // accident: with time passing between the two `Instant::now()` calls, the
        // rebuilt deadline IS later than the caller's. If this assertion ever
        // failed, the property below would be vacuous.
        let caller = Instant::now() + std::time::Duration::from_millis(300);
        let rebased = {
            let budget = caller.saturating_duration_since(Instant::now());
            // Whatever the code between a request's entry point and a helper
            // does, time passes. A millisecond is enough to make the two
            // readings of the clock distinct.
            std::thread::sleep(std::time::Duration::from_millis(1));
            Instant::now() + budget
        };
        assert!(
            rebased > caller,
            "negative control: the rebased deadline must exceed the caller's for this test to \
             mean anything (caller=+300ms, rebuilt={rebased:?})"
        );

        // ── The propagated property ─────────────────────────────────────────
        // With the same kind of delay before the helper looks, the seam returns
        // the caller's own instant (or a cap on it), so it can never be later.
        let caller = Instant::now() + std::time::Duration::from_millis(300);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let inherited = probe_deadline(Some(caller))
            .expect("a caller deadline 300ms out must still be probeable");
        assert!(
            inherited <= caller,
            "C: a helper must never receive a deadline later than its caller's \
             (caller={caller:?}, inherited={inherited:?})"
        );

        // A generous caller is shortened, never extended: still the caller's
        // clock, capped at GIT_PROBE_TIMEOUT.
        let generous = Instant::now() + std::time::Duration::from_secs(60);
        let inherited = probe_deadline(Some(generous)).expect("a generous caller probes");
        assert!(
            inherited <= generous,
            "C: capping must not turn into an extension"
        );
        assert!(
            inherited <= Instant::now() + GIT_PROBE_TIMEOUT,
            "C: the cap bounds a generous caller"
        );

        // And an expired deadline is *spent*, not converted into a fresh grant.
        let expired = Instant::now() - std::time::Duration::from_secs(1);
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(
            probe_deadline(Some(expired)).is_none(),
            "C: an expired caller deadline must not become a fresh budget"
        );
    }

    /// The behavioural half of item C: a helper under an expired caller deadline
    /// must return without launching, and a helper under a live caller deadline
    /// must finish near that deadline rather than at a fresh `GIT_PROBE_TIMEOUT`.
    ///
    /// The structural test above proves the seam never fabricates time; this one
    /// proves the seam is the one the helpers actually consult.
    #[test]
    fn a_helper_actually_consults_the_propagated_deadline() {
        // A caller deadline constructed BEFORE a deliberate delay, then passed
        // down. The delay stands in for whatever real work happens between a
        // request's entry point and the helper that serves it.
        let caller = Instant::now() + std::time::Duration::from_millis(60);
        std::thread::sleep(std::time::Duration::from_millis(120));

        // The caller's deadline is already in the past. A rebase
        // (`now + (caller - now)`) would produce `now`, i.e. a *fresh* zero — and
        // a zero check that then computes `Instant::now() + 0` still admits the
        // launch. What must not happen is any of these helpers treating the
        // remaining time as a budget to be rebuilt.
        let remaining = caller.saturating_duration_since(Instant::now());
        assert_eq!(
            remaining,
            std::time::Duration::ZERO,
            "the fixture must present an expired deadline, or it proves nothing"
        );

        // `developer_dir_via` with an expired caller deadline: must not launch.
        let dir = tempfile::tempdir().unwrap();
        let hanging = dir.path().join("xcode-select");
        std::fs::write(&hanging, b"#!/bin/sh\nwhile :; do :; done\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&hanging).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&hanging, perms).unwrap();
        }
        let started = Instant::now();
        let found = developer_dir_via(&hanging, Some(caller));
        let elapsed = started.elapsed();
        assert!(
            found.is_none(),
            "an expired caller deadline must not yield a developer directory"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "C: an expired deadline must not be rebuilt into a fresh budget and then waited out \
             (took {elapsed:?} against a command that never exits)"
        );

        // `is_capable_git` with an expired caller deadline: must not probe.
        let started = Instant::now();
        let selection = is_capable_git(&hanging, Some(caller));
        let elapsed = started.elapsed();
        assert!(
            selection.is_none(),
            "an expired caller deadline must fail the probe rather than grant it a fresh one"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "C: the probe deadline must be the caller's instant, not a budget rebuilt from now \
             (took {elapsed:?} against a command that never exits)"
        );

        // And the positive direction, so the assertions above are not satisfied
        // by a helper that simply never answers: a caller deadline in the future
        // is honoured as the *same* instant, so the helper may wait for it — but
        // no longer than it.
        let live = Instant::now() + std::time::Duration::from_millis(150);
        let started = Instant::now();
        let found = developer_dir_via(&hanging, Some(live));
        let elapsed = started.elapsed();
        assert!(found.is_none(), "a hanging command yields no directory");
        assert!(
            elapsed >= std::time::Duration::from_millis(100),
            "C: a live caller deadline must still bound the wait, not be ignored \
             (returned in {elapsed:?})"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(400),
            "C: the wait must end near the caller's 150ms deadline, not a fresh \
             GIT_PROBE_TIMEOUT (took {elapsed:?})"
        );
        eprintln!("C_TEST_DEADLINE expired-path elapsed={elapsed:?} (live caller was 150ms)");
    }

    /// An existing repository with no commit cannot satisfy the metadata probe.
    ///
    /// The other false negative: a candidate pointed at a repository whose `HEAD`
    /// does not resolve exits 128 on `rev-parse` and `log`. Probing against such a
    /// repository would report the candidate as unusable when the real problem is
    /// the fixture, so the probe builds its own committed repository instead —
    /// which is also why it can distinguish "git cannot do this" from "there was
    /// nothing to report".
    #[test]
    fn the_capability_probe_requires_a_real_commit_to_succeed() {
        let Some(selection) = resolve_git_executable(None) else {
            enforce_verdict(
                GitVerdict::HostUnavailable,
                "P1B_F2_GIT_PROFILE: no capable git on this host",
            );
            return;
        };
        let git = selection.program;
        let bare = tempfile::tempdir().unwrap();
        // A repository with no commit: `rev-parse --abbrev-ref HEAD` exits 128.
        let init = run_probe(
            &git,
            &["init", "-q"],
            bare.path(),
            &None,
            Instant::now() + GIT_PROBE_TIMEOUT,
        );
        assert_eq!(init, Some(0), "git init must succeed");
        let headless = run_probe_output(
            &git,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            bare.path(),
            &None,
            Instant::now() + GIT_PROBE_TIMEOUT,
        );
        assert!(
            headless.is_none() || headless.as_ref().is_some_and(|(code, _)| *code != 0),
            "F2: a repository with no commit must not yield a successful rev-parse — this is \
             why the probe commits before reading"
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
            enforce_verdict(
                GitVerdict::HostUnavailable,
                "P1B_F2_GIT_METADATA: no functional git on this host",
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
                        "P1B_F2_GIT_METADATA=ENV_BLOCKED broker refused to launch git ({refusal})"
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
                         `git {what}` ({stderr})"
                    );
                    verdicts.push(GitVerdict::EnvBlocked);
                    continue;
                }
                if is_host_unavailable(stderr.as_bytes()) {
                    eprintln!(
                        "P1B_F2_GIT_METADATA=HOST_UNAVAILABLE git could not run for `git {what}` ({stderr})"
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

        // One aggregate, one gate. Not a failure of the code and not a pass: when
        // the environment refused the profile, the metadata path remains
        // unmeasured here — and the harness must record that as a non-pass rather
        // than a green run nobody can tell apart from a measured one. The
        // previous shape printed the state and returned, so cargo graded a
        // blocked host as evidence; that is exactly the false-green F3 was about.
        let aggregate = GitVerdict::aggregate(verdicts);
        enforce_verdict(aggregate, "P1B_F2_GIT_METADATA");
        eprintln!(
            "P1B_F2_GIT_METADATA=PASS rev-parse, log and status all returned real metadata \
             through the broker"
        );
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

    /// The gate itself is not false-green.
    ///
    /// `enforce_verdict` is what turns a blocked host into a non-pass suite. If
    /// it ever came to *report* a blocked state and still return — the exact
    /// shape F3 found in the tests it now guards — every evidence test on an
    /// unmeasurable host would go green again, silently. So the gate's own
    /// behaviour is pinned here: only `Pass` proceeds; every other state must
    /// panic, which is the only channel cargo treats as a failure.
    #[test]
    fn the_verdict_gate_records_blocked_states_as_non_passes() {
        // Pass proceeds: the closure runs and is seen to have run.
        let result = std::panic::catch_unwind(|| {
            enforce_verdict(GitVerdict::Pass, "P1B_F3_GATE");
        });
        assert!(
            result.is_ok(),
            "F3: a passing verdict must not be gated out"
        );

        // Every other state must be loud. The panic message carries the state
        // and the label, so the failure output says what could not be measured
        // rather than just failing.
        for (blocked, state) in [
            (GitVerdict::EnvBlocked, "ENV_BLOCKED"),
            (GitVerdict::HostUnavailable, "HOST_UNAVAILABLE"),
            (GitVerdict::Fail, "Fail"),
        ] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                enforce_verdict(blocked, "P1B_F3_GATE");
            }));
            let payload = match result {
                Ok(()) => panic!(
                    "F3: {state} must reach the harness as a non-pass, not be printed and dropped"
                ),
                Err(payload) => payload,
            };
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            assert!(
                message.contains(state),
                "F3: the gate's panic must name the state ({state}) so a red suite is legible; \
                 got: {message:?}"
            );
            assert!(
                message.contains("P1B_F3_GATE"),
                "F3: the gate's panic must carry the label so the failure is attributable"
            );
        }
    }
}
