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
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::{ManagedChild, SpawnOptions};

mod compiler;

pub use compiler::{CompileError, CompiledProfile, TrustedToolchainRoot};

/// Codex-derived Seatbelt baseline, embedded verbatim.
///
/// Direct copy from `openai/codex` @ `69f7140559180269e2eb8f5be6e0c20eb37b0c85`
/// (Apache-2.0). See `research/spikes/CODEX_SEATBELT_REUSE.md`.
#[cfg(target_os = "macos")]
const CODEX_BASE_POLICY: &str = include_str!("sbpl/codex_base_policy.sbpl");

/// Codex-derived minimum system/runtime allowances, embedded verbatim.
#[cfg(target_os = "macos")]
const CODEX_READ_ONLY_PLATFORM_DEFAULTS: &str =
    include_str!("sbpl/codex_read_only_platform_defaults.sbpl");

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
///
/// # There is no unconfined variant
///
/// Round 2 of this spike carried a `UnconfinedForFidelityTesting` variant
/// plus a public `spawn_unconfined_for_fidelity_testing` escape hatch, on the
/// theory that refusing it inside `spawn` was enough. It was not: the variant
/// and the method were both `pub`, so any production caller in any crate could
/// obtain a completely unrestricted child by asking for one. A safety
/// property that depends on a runtime check inside one function is not a
/// safety property.
///
/// The variant is now **gone from the type**, not merely refused. Tests that
/// need a permissive profile construct one through a `#[cfg(test)]`-only
/// helper that does not exist in a release build, so the escape hatch is not
/// rejected at runtime — it is *absent from the compiled program*. See
/// `execution_broker::testing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxPlan {
    /// The real thing: an allow-list. Nothing is reachable except the roots
    /// named here, on top of the minimum system allowances every process
    /// needs in order to start.
    Confined {
        /// Subtrees the task may read **and** write.
        writable_roots: Vec<PathBuf>,
        /// Subtrees the task may read but not modify.
        readable_roots: Vec<PathBuf>,
        /// Whether the task may create sockets / reach the network.
        network: NetworkPolicy,
    },
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
    /// The plan could not be compiled. Carries the compiler's own explanation
    /// so the refusal names the actual root that failed.
    PlanNotCompilable(String),
    /// The spec is internally inconsistent and cannot be executed as stated.
    SpecInvalid(&'static str),
    /// The requested working directory is not inside the authority the plan
    /// granted.
    ///
    /// This is a **fail-closed refusal before any process exists**, not a
    /// warning. See [`ExecutionBroker::check_cwd`] for why the broker refuses
    /// rather than quietly widening the profile to include the cwd.
    CwdOutsideSandboxRoots {
        /// The working directory as the caller stated it.
        cwd: PathBuf,
        /// Why it is not covered by the plan.
        reason: String,
    },
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
            Self::PlanNotCompilable(why) => write!(f, "sandbox plan refused: {why}"),
            Self::SpecInvalid(why) => write!(f, "invalid spawn spec: {why}"),
            Self::CwdOutsideSandboxRoots { cwd, reason } => write!(
                f,
                "spawn refused: working directory {} is outside this action's \
                 sandbox roots: {reason}",
                cwd.display()
            ),
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

    /// Compile this plan into a profile and its launcher definitions.
    ///
    /// Thin re-export of [`compiler::compile`] so callers holding a plan do not
    /// need to know the compiler is a separate module. Paths travel to the
    /// kernel as argv parameters, never as profile text — see
    /// [`compiler`] for why that matters.
    #[cfg(target_os = "macos")]
    pub fn compile(
        &self,
        toolchain_roots: &[TrustedToolchainRoot],
    ) -> Result<CompiledProfile, CompileError> {
        compiler::compile(self, toolchain_roots)
    }
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

    /// Refuse a spec whose working directory the plan does not cover.
    ///
    /// # The invariant
    ///
    /// A confined action may only `chdir` into a directory its own profile can
    /// reach: the canonicalized `cwd` must be inside a **writable** root or a
    /// **readable** root. Anything else is refused, before any process exists.
    ///
    /// # Why the broker refuses instead of widening the profile
    ///
    /// The obvious "fix" — append the cwd to `readable_roots` so `chdir` can
    /// succeed — is exactly the authority widening this module exists to
    /// prevent. A cwd is attacker-influenced in the general case (it can come
    /// from a model-authored action, or from a path the user merely mentioned),
    /// and a directory need only *exist* for the grant to be minted. Turning
    /// "the action asked for a directory" into "the action may read that
    /// directory and everything under it" is a read grant nobody reviewed.
    ///
    /// The failure this prevents is observable: `sandbox-exec` sets the child's
    /// cwd *after* the profile is applied, so a child launched in a directory
    /// outside every root starts successfully and then operates in a directory
    /// it is not allowed to read — reporting
    /// `getcwd: cannot access parent directories` from every tool that tries to
    /// resolve its own path.
    ///
    /// # Why canonicalization, not string prefix matching
    ///
    /// On macOS `/tmp` is a symlink to `/private/tmp`, so a literal prefix
    /// comparison against a non-canonical root both misses legitimate matches
    /// and, worse, can be defeated by a symlink whose *literal* path is inside a
    /// root while its *target* is outside it. Both sides are canonicalized and
    /// then compared component-wise, so the check asks "is this the same
    /// directory", not "does this string start with that string".
    pub fn check_cwd(&self, spec: &SpawnSpec) -> Result<(), BrokerError> {
        let cwd = spec
            .cwd
            .canonicalize()
            .map_err(|e| BrokerError::CwdOutsideSandboxRoots {
                cwd: spec.cwd.clone(),
                reason: format!("working directory does not resolve to a real directory: {e}"),
            })?;

        if !cwd.is_dir() {
            return Err(BrokerError::CwdOutsideSandboxRoots {
                cwd: spec.cwd.clone(),
                reason: "working directory is not a directory".to_string(),
            });
        }

        let (writable, readable) = match &spec.plan {
            SandboxPlan::Confined {
                writable_roots,
                readable_roots,
                ..
            } => (writable_roots, readable_roots),
        };

        let covered = writable
            .iter()
            .chain(readable.iter())
            .filter_map(|root| root.canonicalize().ok())
            .any(|root| cwd == root || cwd.starts_with(&root));

        if covered {
            return Ok(());
        }

        // Name the roots that were tried. An error that only says "outside" is
        // an error the caller cannot act on without guessing.
        let mut tried: Vec<String> = Vec::new();
        for (label, roots) in [("writable", writable), ("readable", readable)] {
            for root in roots {
                let shown = root.canonicalize().unwrap_or_else(|_| root.clone());
                tried.push(format!("{label} {}", shown.display()));
            }
        }
        Err(BrokerError::CwdOutsideSandboxRoots {
            cwd: spec.cwd.clone(),
            reason: if tried.is_empty() {
                "the plan grants no filesystem roots at all".to_string()
            } else {
                format!(
                    "resolved to {}, which is not inside any of: {}",
                    cwd.display(),
                    tried.join(", ")
                )
            },
        })
    }

    /// Build the final `Command` for `spec` without spawning it.
    ///
    /// Deliberately **`pub(crate)`, not `pub`**. Round 2 made this public so
    /// tests could inspect it, which also made it a public primitive for
    /// assembling a `sandbox-exec` invocation by hand — one more way to reach
    /// an under-constrained child.
    ///
    /// It is also absent from a **release** build. Nothing in production calls
    /// it: `spawn_with_toolchain` builds the command and immediately spawns it,
    /// so there is no reason for the intermediate value to exist outside tests.
    /// `#[cfg(test)]` is what makes that true rather than merely intended.
    #[cfg(target_os = "macos")]
    #[cfg(test)]
    pub(crate) fn build_command(&self, spec: &SpawnSpec) -> Result<Command, BrokerError> {
        self.build_command_with_toolchain(spec, &[])
    }

    /// As [`Self::build_command`], plus the extra read-only prefixes the action
    /// needs *in order to start at all* — a Homebrew `node`, for instance.
    #[cfg(target_os = "macos")]
    pub(crate) fn build_command_with_toolchain(
        &self,
        spec: &SpawnSpec,
        toolchain_roots: &[TrustedToolchainRoot],
    ) -> Result<Command, BrokerError> {
        // Render first: an inexpressible plan must fail before anything exists.
        let compiled = spec
            .plan
            .compile(toolchain_roots)
            .map_err(|e| BrokerError::PlanNotCompilable(e.to_string()))?;
        if spec.program.as_os_str().is_empty() {
            return Err(BrokerError::SpecInvalid("program is empty"));
        }
        // Then the cwd invariant, still before any process exists. Order
        // matters only in that an inexpressible plan is the more fundamental
        // complaint; both fail closed.
        self.check_cwd(spec)?;

        let mut command = Command::new(SANDBOX_EXEC);
        command.arg("-p").arg(&compiled.sbpl);
        // Path parameters travel as argv, never as profile text.
        for definition in &compiled.definitions {
            command.arg(format!("-D{definition}"));
        }
        // `--` ends sandbox-exec's own option parsing so a program path that
        // begins with `-` cannot be read as one of its flags.
        command.arg("--").arg(&spec.program);
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
    /// This is the **only** way to obtain a sandboxed child. There is no
    /// unrestricted counterpart, and none can be added without a `#[cfg(test)]`
    /// gate — see the note on [`SandboxPlan`].
    #[cfg(target_os = "macos")]
    pub fn spawn(&self, spec: &SpawnSpec) -> Result<ManagedChild, BrokerError> {
        self.spawn_with_toolchain(spec, &[])
    }

    /// As [`Self::spawn`], plus the read-only prefixes the action needs in
    /// order to start.
    ///
    /// A deny-default profile must be able to *execute* the interpreter the
    /// action asked for. On a machine whose toolchain lives in
    /// `/opt/homebrew`, the Codex-derived platform defaults — which stop at
    /// fixed system prefixes — are not enough, and without this the action
    /// fails to start rather than running restricted.
    ///
    /// # The roots are [`TrustedToolchainRoot`], not paths
    ///
    /// This parameter widens what the action can read, past the roots the plan
    /// named. It was previously `&[PathBuf]`, which meant a caller could pass
    /// `"/"` and obtain a profile granting `(subpath "/")` — the whole
    /// filesystem, readable. Every check that should have caught that passed,
    /// because `/` is absolute and is not inside `$HOME`.
    ///
    /// The type removes the ability to express it. There is no public
    /// constructor and the field is private, so a caller can only pass roots the
    /// host derived from an executable it resolved, inside a recognised
    /// toolchain prefix. To grant a prefix, resolve the executable:
    ///
    /// ```ignore
    /// let node = TrustedToolchainRoot::resolve(Path::new("/opt/homebrew/bin/node"))?;
    /// broker.spawn_with_toolchain(&spec, &[node])?;
    /// ```
    #[cfg(target_os = "macos")]
    pub fn spawn_with_toolchain(
        &self,
        spec: &SpawnSpec,
        toolchain_roots: &[TrustedToolchainRoot],
    ) -> Result<ManagedChild, BrokerError> {
        let mut command = self.build_command_with_toolchain(spec, toolchain_roots)?;
        ManagedChild::spawn_with_options(&mut command, SpawnOptions::new())
            .map_err(BrokerError::Launch)
    }

    /// No supported backend on this platform: refuse rather than spawn bare.
    #[cfg(not(target_os = "macos"))]
    #[cfg(test)]
    pub(crate) fn build_command(&self, _spec: &SpawnSpec) -> Result<Command, BrokerError> {
        Err(BrokerError::UnsupportedPlatform)
    }

    /// No supported backend on this platform: refuse rather than spawn bare.
    #[cfg(not(target_os = "macos"))]
    pub fn spawn(&self, _spec: &SpawnSpec) -> Result<ManagedChild, BrokerError> {
        Err(BrokerError::UnsupportedPlatform)
    }
}

/// Test-only helpers.
///
/// The whole module is `#[cfg(test)]`, so nothing here exists in a release
/// build: there is no function to call, no variant to construct, and no
/// environment variable, flag, or secret handshake that unlocks an
/// unrestricted spawn. A `cargo build --release` of this crate contains no
/// unrestricted execution path at all.
///
/// This exists so the command-fidelity tests can run a real `sandbox-exec`
/// child with a permissive profile on a host whose kernel refuses restrictive
/// ones. Without it those tests would either be skipped on such a host — in
/// which case they would establish nothing — or would bypass the broker, which
/// is the thing under test.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::execution_broker::compiler::CODEX_TEST_PERMISSIVE_PROFILE;

    /// A profile that permits everything, for tests that are about command
    /// construction rather than confinement.
    ///
    /// Compiled by the same code path as a real profile, so a fidelity test
    /// still exercises the launcher, the argv assembly, and the process
    /// lifecycle. Only the restrictions are absent.
    pub(crate) const PERMISSIVE_PROFILE: &str = CODEX_TEST_PERMISSIVE_PROFILE;

    /// Build a permissive launcher invocation, for fidelity tests only.
    ///
    /// `pub(crate)` **and** `#[cfg(test)]`: the second is what makes this
    /// absent from a release build. The first keeps it out of the crate's
    /// public API even in a test build of the library.
    pub(crate) fn build_permissive_command(spec: &SpawnSpec) -> Result<Command, BrokerError> {
        if spec.program.as_os_str().is_empty() {
            return Err(BrokerError::SpecInvalid("program is empty"));
        }
        let mut command = Command::new(SANDBOX_EXEC);
        command
            .arg("-p")
            .arg(PERMISSIVE_PROFILE)
            .arg("--")
            .arg(&spec.program);
        command.args(&spec.args);
        command.current_dir(&spec.cwd);

        match spec.env {
            EnvPolicy::Empty => {
                command.env_clear();
            }
            EnvPolicy::Minimal => {
                command.env_clear();
                command.env("PATH", "/usr/bin:/bin");
                command.env("HOME", &spec.cwd);
            }
            EnvPolicy::Inherit => {
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

    /// Spawn under the permissive profile, for fidelity tests only.
    pub(crate) fn spawn_permissive(spec: &SpawnSpec) -> Result<ManagedChild, BrokerError> {
        let mut command = build_permissive_command(spec)?;
        ManagedChild::spawn_with_options(&mut command, SpawnOptions::new())
            .map_err(BrokerError::Launch)
    }
}

#[cfg(test)]
#[path = "fidelity_tests.rs"]
mod fidelity_tests;
