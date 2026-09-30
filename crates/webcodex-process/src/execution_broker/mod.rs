// SPDX-License-Identifier: Apache-2.0
//! Per-action sandbox execution broker.
//!
//! # What this module is for
//!
//! Each model-triggered action gets **its own** sandbox profile, and the task's
//! descendants inherit that profile, while the runner that decided the
//! permission stays outside every profile.
//!
//! This is the alternative to a runner-level UNION sandbox. A union profile has
//! one specific failure mode: **an action that forgets to narrow inherits the
//! union and is over-permissioned by default.** The failure is silent and is
//! worst where it matters most — a spawn surface nobody remembered to route.
//!
//! ```text
//! WebCodex runner          = TRUSTED CONTROL PLANE (never sandboxed here)
//!   -> (SecurityBroker decides ALLOW / ASK / DENY — not in this spike)
//!     -> SpawnSpec { program, args, cwd, env, stdio, plan }
//!       -> ExecutionBroker::spawn(&spec)
//!         -> platform sandbox (sandbox-exec + SBPL profile)
//!           -> task process
//!             -> descendants inherit THIS profile
//! ```
//!
//! # Why `SpawnSpec` and not `&mut Command`
//!
//! Round 1 of this spike took a `&mut Command` and copied three fields out of
//! it (`get_program`, `get_args`, `current_dir`). That silently discarded
//! everything else the caller had already configured — `stdin`/`stdout`/
//! `stderr`, and the whole environment. A caller who set
//! `cmd.stdout(Stdio::piped())` got a child whose stdout was **not** piped,
//! with no error anywhere. Patching that by reading more fields off `Command`
//! is the wrong direction: `Command` has no API to enumerate its own state,
//! so every field added to that list is a field that can be forgotten.
//!
//! `SpawnSpec` inverts the ownership. The caller states the execution
//! completely, as a value, and the broker builds the final `Command` from that
//! value. Anything the caller did not state does not happen — including the
//! environment, which is *never* inherited implicitly (see
//! [`EnvPolicy`]).
//!
//! # What is NOT claimed
//!
//! - **No production code calls this yet.** It is a seam proposal, exercised by
//!   tests only. `run_shell` is not sandboxed by this branch.
//! - The macOS backend is `/usr/bin/sandbox-exec`, which is **not a supported
//!   third-party API** — see [`BACKEND_STATUS`].
//! - Restrictive-profile enforcement could not be measured on the spike host;
//!   see `research/spikes/EXECUTION_BROKER_SPIKE_2_RESULTS.md`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{ManagedChild, SpawnOptions};

/// Absolute path to Apple's profile-enforcing launcher.
///
/// # Backend status
///
/// `sandbox-exec` is a private, unsupported mechanism. Apple has publicly
/// deprecated the Seatbelt profile language and does not document or support
/// third-party use of it, while providing no replacement for restricting a
/// child process. It is used here because it is the facility Codex itself
/// reaches for, not because it is a stable contract. See [`BACKEND_STATUS`].
#[cfg(target_os = "macos")]
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Maintenance standing of the macOS backend this module uses.
pub const BACKEND_STATUS: BackendStatus = BackendStatus {
    deprecated: true,
    unsupported_for_third_party_custom_policy: true,
    used_by_codex_as_pragmatic_backend: true,
    replacement_available: false,
};

/// Whether the chosen backend is a contract we can depend on.
///
/// Recorded as data rather than prose so a future backend swap is a visible
/// diff, and so no downstream reader can mistake "works on our host" for
/// "supported".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendStatus {
    /// The profile language is deprecated by its vendor.
    pub deprecated: bool,
    /// Using it to express a custom third-party policy is not supported.
    pub unsupported_for_third_party_custom_policy: bool,
    /// The upstream project this spike follows uses it anyway.
    pub used_by_codex_as_pragmatic_backend: bool,
    /// A supported replacement exists on this platform.
    pub replacement_available: bool,
}

impl BackendStatus {
    /// One-line standing, for reports and error text.
    pub fn summary(&self) -> String {
        format!(
            "DEPRECATED={} UNSUPPORTED_FOR_THIRD_PARTY_CUSTOM_POLICY={} \
             USED_BY_CODEX_AS_PRAGMATIC_BACKEND={} REPLACEMENT_AVAILABLE={}",
            self.deprecated,
            self.unsupported_for_third_party_custom_policy,
            self.used_by_codex_as_pragmatic_backend,
            self.replacement_available
        )
    }
}

/// One action's authority, expressed as filesystem reach and network reach.
///
/// A `SandboxPlan` is a **value**, not a policy: it carries no decision about
/// whether the action is allowed, only what the action may touch once it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxPlan {
    /// The real thing: an allow-list. Nothing is reachable except the roots
    /// named here.
    Confined {
        /// Subtrees the task may read **and** write.
        writable_roots: Vec<PathBuf>,
        /// Subtrees the task may read but not modify.
        readable_roots: Vec<PathBuf>,
        /// Whether the task may create sockets / reach the network.
        network: NetworkPolicy,
    },
    /// A profile with **no restrictions at all** — renders to a bare
    /// `(allow default)`.
    ///
    /// This exists for one reason: the command-fidelity tests must run for real
    /// on a host whose kernel refuses restrictive profiles, and they must do so
    /// through the same launcher, the same spec, and the same process handling
    /// as a confined action. A fidelity test that skipped the broker would
    /// establish nothing.
    ///
    /// It is deliberately **not reachable from the normal spawn path**:
    /// [`ExecutionBroker::spawn`] refuses it. Only
    /// [`ExecutionBroker::spawn_unconfined_for_fidelity_testing`] will honour
    /// it, so no production caller can obtain an unconfined child by
    /// constructing a plan. If a future test needs a real restriction it should
    /// use [`SandboxPlan::Confined`] on a host that supports it.
    UnconfinedForFidelityTesting,
}

/// Per-action network authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// Deny all network access.
    Deny,
    /// Allow network access. Refused by this spike: no proxy backend exists, so
    /// the variant is present to make the omission visible rather than to imply
    /// a working allow path.
    Allow,
}

/// How the spawned action's environment is built.
///
/// The default is [`EnvPolicy::Minimal`], and that default is the point: a
/// sandboxed child of the runner must not inherit the runner's credentials
/// merely because the caller forgot to mention them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvPolicy {
    /// Start from an empty environment and add only what the caller states,
    /// plus a `PATH` and a `HOME`. **This is the default.**
    Minimal,
    /// Start from the broker process's own environment. Explicit opt-in only.
    Inherit,
    /// Start from an empty environment with nothing added — not even `PATH`.
    Empty,
}

impl Default for EnvPolicy {
    fn default() -> Self {
        Self::Minimal
    }
}

/// What the action may do with its three standard streams.
///
/// `Inherit` means "whatever the broker process has". It is a separate variant
/// rather than a boolean because the interesting case — a caller that asked for
/// a pipe — must be distinguishable from one that did not ask at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamPolicy {
    /// No stream. Equivalent to [`Stdio::null`].
    Null,
    /// A pipe the caller drains through [`ManagedChild::child_mut`].
    Piped,
    /// The broker process's own stream.
    Inherit,
}

/// The complete description of one action's execution.
///
/// The broker builds the final `Command` from this value. It is intentionally
/// not a builder with forty optional setters: the fields are the ones that
/// change what a sandboxed process can do, and a caller must state each one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    /// Path or bare name of the program to run.
    pub program: PathBuf,
    /// Arguments after the program.
    pub args: Vec<OsString>,
    /// Working directory for the action.
    pub cwd: PathBuf,
    /// Environment construction rule.
    pub env: EnvPolicy,
    /// Variables to add, used with [`EnvPolicy::Minimal`] and
    /// [`EnvPolicy::Inherit`].
    pub env_vars: BTreeMap<String, OsString>,
    /// Variables to remove from an inherited environment.
    pub env_remove: Vec<String>,
    /// Standard input.
    pub stdin: StreamPolicy,
    /// Standard output.
    pub stdout: StreamPolicy,
    /// Standard error.
    pub stderr: StreamPolicy,
    /// This action's sandbox authority.
    pub plan: SandboxPlan,
}

impl SpawnSpec {
    /// A minimal spec: run `program` under `plan` in `cwd`, no streams, minimal
    /// environment. Everything else is opt-in.
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>, plan: SandboxPlan) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: EnvPolicy::default(),
            env_vars: BTreeMap::new(),
            env_remove: Vec::new(),
            stdin: StreamPolicy::Null,
            stdout: StreamPolicy::Null,
            stderr: StreamPolicy::Null,
            plan,
        }
    }

    /// Add one argument.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set the environment construction rule.
    pub fn env(mut self, policy: EnvPolicy) -> Self {
        self.env = policy;
        self
    }

    /// Add one environment variable.
    pub fn env_var(mut self, key: impl Into<String>, value: impl Into<OsString>) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Remove one variable from an inherited environment.
    pub fn env_remove(mut self, key: impl Into<String>) -> Self {
        self.env_remove.push(key.into());
        self
    }

    /// Set standard input.
    pub fn stdin(mut self, policy: StreamPolicy) -> Self {
        self.stdin = policy;
        self
    }

    /// Set standard output.
    pub fn stdout(mut self, policy: StreamPolicy) -> Self {
        self.stdout = policy;
        self
    }

    /// Set standard error.
    pub fn stderr(mut self, policy: StreamPolicy) -> Self {
        self.stderr = policy;
        self
    }
}

/// Why a brokered spawn could not be started.
///
/// Constructing a profile and applying it are separate steps on purpose: a
/// caller that cannot build a valid profile must fail **before** a process
/// exists, never after.
#[derive(Debug)]
pub enum BrokerError {
    /// This host has no supported sandbox backend.
    UnsupportedPlatform,
    /// The plan is not expressible as a profile, so the action is refused.
    /// Refusing here is the point: an inexpressible plan must never fall back
    /// to an unconfined spawn.
    PlanRefused(&'static str),
    /// The spec is internally inconsistent and cannot be executed as stated.
    SpecInvalid(&'static str),
    /// The launcher could not be started.
    Launch(io::Error),
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform => write!(
                f,
                "no sandbox backend for this platform (spike supports macOS only)"
            ),
            Self::PlanRefused(why) => write!(f, "sandbox plan refused: {why}"),
            Self::SpecInvalid(why) => write!(f, "invalid spawn spec: {why}"),
            Self::Launch(e) => write!(f, "sandbox launcher failed: {e}"),
        }
    }
}

impl std::error::Error for BrokerError {}

impl SandboxPlan {
    /// A confined plan for "touch nothing but `writable_roots`".
    pub fn read_write_in(roots: &[PathBuf]) -> Self {
        Self::Confined {
            writable_roots: roots.to_vec(),
            readable_roots: Vec::new(),
            network: NetworkPolicy::Deny,
        }
    }

    /// Render this plan as an SBPL profile string.
    ///
    /// A [`SandboxPlan::Confined`] plan renders an **allow-list**: nothing is
    /// permitted except the roots it names, and everything else falls through
    /// to the implicit deny. This is the property that makes per-action
    /// profiles independent — a second plan naming a different root set cannot
    /// read the first plan's data.
    pub fn to_sbpl(&self) -> Result<String, BrokerError> {
        let (writable_roots, readable_roots, network) = match self {
            Self::Confined {
                writable_roots,
                readable_roots,
                network,
            } => (writable_roots, readable_roots, network),
            Self::UnconfinedForFidelityTesting => {
                return Ok(String::from("(version 1)\n(allow default)\n"))
            }
        };
        if writable_roots.is_empty() && readable_roots.is_empty() {
            // A plan with no roots would compile to "deny everything", which is
            // a plausible-looking but almost certainly unintended action.
            return Err(BrokerError::PlanRefused(
                "plan grants no filesystem access; refusing rather than spawning a \
                 process that can do nothing",
            ));
        }
        let mut sbpl = String::from("(version 1)\n(allow default)\n(deny file-read*)\n");
        for root in readable_roots.iter().chain(writable_roots.iter()) {
            sbpl.push_str(&format!(
                "(allow file-read* (subpath \"{}\"))\n",
                escape(root)
            ));
        }
        sbpl.push_str("(deny file-write*)\n");
        for root in writable_roots {
            sbpl.push_str(&format!(
                "(allow file-write* (subpath \"{}\"))\n",
                escape(root)
            ));
        }
        match network {
            NetworkPolicy::Deny => sbpl.push_str("(deny network*)\n"),
            // No proxy backend exists, so an allow plan is refused rather than
            // silently downgraded. Making this a hard error is what stops
            // `Allow` from reading as "network works".
            NetworkPolicy::Allow => {
                return Err(BrokerError::PlanRefused(
                    "network-allow plans require a proxy backend this spike does not implement",
                ))
            }
        }
        Ok(sbpl)
    }
}

fn escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn stdio(policy: StreamPolicy) -> Stdio {
    match policy {
        StreamPolicy::Null => Stdio::null(),
        StreamPolicy::Piped => Stdio::piped(),
        StreamPolicy::Inherit => Stdio::inherit(),
    }
}

/// Spawns one action under that action's own profile.
///
/// The broker is a stateless function of a [`SpawnSpec`]. It holds no authority
/// of its own and is never itself sandboxed, which is what keeps the control
/// plane able to create a *different* profile for the next action.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionBroker;

impl ExecutionBroker {
    /// Create a broker.
    pub fn new() -> Self {
        Self
    }

    /// Build the final `Command` for `spec` without spawning it.
    ///
    /// Exposed so a caller (or a test) can inspect exactly what would be
    /// executed. The broker owns command construction; this is the seam where
    /// that ownership is observable.
    #[cfg(target_os = "macos")]
    pub fn build_command(&self, spec: &SpawnSpec) -> Result<Command, BrokerError> {
        // Render first: an inexpressible plan must fail before anything exists.
        let sbpl = spec.plan.to_sbpl()?;
        if spec.program.as_os_str().is_empty() {
            return Err(BrokerError::SpecInvalid("program is empty"));
        }

        let mut command = Command::new(SANDBOX_EXEC);
        command.arg("-p").arg(&sbpl).arg(&spec.program);
        command.args(&spec.args);
        command.current_dir(&spec.cwd);

        match spec.env {
            EnvPolicy::Empty => {
                command.env_clear();
            }
            EnvPolicy::Minimal => {
                command.env_clear();
                // A sandboxed action gets a working PATH and a HOME that points
                // at its own working directory, and nothing else. Anything the
                // caller needs beyond this must be named in `env_vars`.
                command.env("PATH", "/usr/bin:/bin");
                command.env("HOME", &spec.cwd);
            }
            EnvPolicy::Inherit => {
                // `Command::env_remove` is per-key, so an inherited environment
                // is scrubbed one variable at a time.
                for key in &spec.env_remove {
                    command.env_remove(key);
                }
            }
        }
        for (key, value) in &spec.env_vars {
            command.env(key, value);
        }

        command.stdin(stdio(spec.stdin));
        command.stdout(stdio(spec.stdout));
        command.stderr(stdio(spec.stderr));
        Ok(command)
    }

    /// Apply `spec`'s plan and spawn it, returning the managed child.
    ///
    /// On macOS the profile is applied by re-executing the command under
    /// `/usr/bin/sandbox-exec`, because Seatbelt profiles are inherited across
    /// `exec` and cannot be attached to an already-running process. The
    /// resulting child is a `ManagedChild` like any other, so process-group
    /// ownership, `wait`, and `terminate_tree` are unchanged.
    ///
    /// A [`SandboxPlan::UnconfinedForFidelityTesting`] spec is **refused
    /// here**. That variant exists only so the fidelity tests can exercise the
    /// real launcher on a host that will not apply a restrictive profile, and
    /// it must never be reachable from a production call site.
    #[cfg(target_os = "macos")]
    pub fn spawn(&self, spec: &SpawnSpec) -> Result<ManagedChild, BrokerError> {
        if matches!(spec.plan, SandboxPlan::UnconfinedForFidelityTesting) {
            return Err(BrokerError::SpecInvalid(
                "unconfined plans are for fidelity tests only; \
                 use spawn_unconfined_for_fidelity_testing",
            ));
        }
        let mut command = self.build_command(spec)?;
        ManagedChild::spawn_with_options(&mut command, SpawnOptions::new())
            .map_err(BrokerError::Launch)
    }

    /// Spawn without restrictions, for the command-fidelity tests only.
    ///
    /// This still goes through `/usr/bin/sandbox-exec` — with `(allow
    /// default)` — so the child has a real launcher as its parent and the
    /// process tree is real. Only the restrictions are absent. The name is
    /// deliberately awkward: this is not a capability a caller should want, and
    /// [`ExecutionBroker::spawn`] will not honour such a plan.
    #[cfg(target_os = "macos")]
    pub fn spawn_unconfined_for_fidelity_testing(
        &self,
        spec: &SpawnSpec,
    ) -> Result<ManagedChild, BrokerError> {
        let mut command = self.build_command(spec)?;
        ManagedChild::spawn_with_options(&mut command, SpawnOptions::new())
            .map_err(BrokerError::Launch)
    }

    /// No supported backend on this platform: refuse rather than spawn bare.
    #[cfg(not(target_os = "macos"))]
    pub fn build_command(&self, _spec: &SpawnSpec) -> Result<Command, BrokerError> {
        Err(BrokerError::UnsupportedPlatform)
    }

    /// No supported backend on this platform: refuse rather than spawn bare.
    #[cfg(not(target_os = "macos"))]
    pub fn spawn(&self, _spec: &SpawnSpec) -> Result<ManagedChild, BrokerError> {
        Err(BrokerError::UnsupportedPlatform)
    }

    /// No supported backend on this platform: refuse rather than spawn bare.
    #[cfg(not(target_os = "macos"))]
    pub fn spawn_unconfined_for_fidelity_testing(
        &self,
        _spec: &SpawnSpec,
    ) -> Result<ManagedChild, BrokerError> {
        Err(BrokerError::UnsupportedPlatform)
    }
}
