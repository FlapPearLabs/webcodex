// SPDX-License-Identifier: Apache-2.0
//! Per-action sandbox execution broker (SPIKE, round 34).
//!
//! # Scope of this spike
//!
//! This module exists to answer one question empirically: **can WebCodex give
//! each model-triggered action its own sandbox profile, and have the task's
//! descendants inherit it, without confining the control plane?**
//!
//! # Why this is not the runner-level UNION profile
//!
//! Rounds 29-33 of the architecture study converged on confining
//! `webcodex-runner` itself to the union of every profile any action could need.
//! That design has a specific failure mode: **an action that forgets to narrow
//! inherits the union and is over-permissioned by default.** The failure is
//! silent, and it is worst exactly where it matters most — a new spawn surface
//! nobody remembered to route through the broker.
//!
//! This module takes the opposite shape:
//!
//! ```text
//! WebCodex runner          = TRUSTED CONTROL PLANE (never sandboxed here)
//!   -> SecurityBroker decides ALLOW / ASK / DENY
//!     -> SandboxPlan (per action)
//!       -> ExecutionBroker::spawn(command, cwd, plan)
//!         -> platform sandbox (sandbox-exec + SBPL profile)
//!           -> task process
//!             -> descendants inherit THIS profile
//! ```
//!
//! Two actions therefore have independent authority: profile A cannot read what
//! profile B is allowed to read, and the runner is never placed inside either
//! profile, so it can always build the next one.
//!
//! # What is NOT claimed
//!
//! - This is **one** integration point on **one** model-triggered path
//!   (`run_shell` -> `ManagedChild::spawn`). It is not whole-surface coverage.
//! - The platform backend is **macOS Seatbelt via `/usr/bin/sandbox-exec`**, and
//!   it was exercised only as far as the host permits (see
//!   `research/spikes/EXECUTION_BROKER_SPIKE_RESULTS.md`).
//! - No other `Command::spawn` site is routed here. See the normalization table.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{ManagedChild, SpawnOptions};

/// Absolute path to Apple's profile-enforcing launcher.
#[cfg(target_os = "macos")]
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// One action's authority, expressed as filesystem reach and network reach.
///
/// A `SandboxPlan` is a **value**, not a policy: it carries no decision about
/// whether the action is allowed, only what the action may touch once it is.
/// Construction happens after `SecurityBroker` has decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPlan {
    /// Subtree the task may read **and** write.
    pub writable_roots: Vec<PathBuf>,
    /// Subtrees the task may read but not modify.
    pub readable_roots: Vec<PathBuf>,
    /// Whether the task may create sockets / reach the network.
    pub network: NetworkPolicy,
}

/// Per-action network authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// Deny all network access.
    Deny,
    /// Allow network access. Only honour this when a proxy profile exists; the
    /// spike does not implement one, so this variant exists to make the
    /// omission visible rather than to imply a working allow path.
    Allow,
}

/// Why a brokered spawn could not be started.
///
/// Constructing a plan and applying it are separate steps on purpose: a caller
/// that cannot build a valid profile must fail **before** a process exists,
/// never after.
#[derive(Debug)]
pub enum BrokerError {
    /// This host has no supported sandbox backend.
    UnsupportedPlatform,
    /// The plan is not expressible as a profile, so the action is refused.
    /// Refusing here is the point: an inexpressible plan must not fall back to
    /// an unconfined spawn.
    PlanRefused(&'static str),
    /// The launcher could not be started.
    Launch(io::Error),
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                write!(
                    f,
                    "no sandbox backend for this platform (spike supports macOS only)"
                )
            }
            Self::PlanRefused(why) => write!(f, "sandbox plan refused: {why}"),
            Self::Launch(e) => write!(f, "sandbox launcher failed: {e}"),
        }
    }
}

impl std::error::Error for BrokerError {}

impl SandboxPlan {
    /// A plan for "touch nothing but `writable_roots`".
    pub fn read_write_in(roots: &[PathBuf]) -> Self {
        Self {
            writable_roots: roots.to_vec(),
            readable_roots: Vec::new(),
            network: NetworkPolicy::Deny,
        }
    }

    /// Render this plan as an SBPL profile string.
    ///
    /// Uses an **allow-list**: nothing is permitted except the roots this plan
    /// names, and everything else falls through to the implicit deny. This is
    /// the property that makes per-action profiles independent — a second plan
    /// naming a different root set cannot read the first plan's data.
    pub fn to_sbpl(&self) -> Result<String, BrokerError> {
        if self.writable_roots.is_empty() && self.readable_roots.is_empty() {
            // A plan with no roots would compile to "deny everything", which is
            // a plausible-looking but almost certainly unintended action.
            return Err(BrokerError::PlanRefused(
                "plan grants no filesystem access; refusing rather than spawning a \
                 process that can do nothing",
            ));
        }
        let mut sbpl = String::from("(version 1)\n(allow default)\n(deny file-read*)\n");
        for root in self.readable_roots.iter().chain(self.writable_roots.iter()) {
            sbpl.push_str(&format!(
                "(allow file-read* (subpath \"{}\"))\n",
                escape(root)
            ));
        }
        sbpl.push_str("(deny file-write*)\n");
        for root in &self.writable_roots {
            sbpl.push_str(&format!(
                "(allow file-write* (subpath \"{}\"))\n",
                escape(root)
            ));
        }
        match self.network {
            NetworkPolicy::Deny => sbpl.push_str("(deny network*)\n"),
            // The spike has no proxy, so an allow plan is refused rather than
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

/// Spawns one action under that action's own profile.
///
/// The broker is a stateless function of `(command, cwd, plan)`. It holds no
/// authority of its own and is never itself sandboxed, which is what keeps the
/// control plane able to create a *different* profile for the next action.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionBroker;

impl ExecutionBroker {
    /// Create a broker.
    pub fn new() -> Self {
        Self
    }

    /// Apply `plan` to `command` and spawn it, returning the managed child.
    ///
    /// On macOS the profile is applied by re-executing the command under
    /// `/usr/bin/sandbox-exec`, because Seatbelt profiles are inherited across
    /// `exec` and cannot be attached to an already-running process. The
    /// resulting child is a `ManagedChild` like any other, so process-group
    /// ownership, `wait`, and `terminate_tree` are unchanged.
    #[cfg(target_os = "macos")]
    pub fn spawn(
        &self,
        command: &mut Command,
        cwd: &Path,
        plan: &SandboxPlan,
    ) -> Result<ManagedChild, BrokerError> {
        let sbpl = plan.to_sbpl()?;
        let program = command.get_program();
        let mut launcher = Command::new(SANDBOX_EXEC);
        launcher.arg("-p").arg(&sbpl).arg(program);
        launcher.args(command.get_args());
        launcher.current_dir(cwd);
        launcher.env_clear();
        // A sandboxed task gets a minimal environment: the plan is the
        // authority, not the inherited shell session.
        launcher.env("PATH", "/usr/bin:/bin");
        launcher.env("HOME", cwd);
        ManagedChild::spawn_with_options(&mut launcher, SpawnOptions::new())
            .map_err(BrokerError::Launch)
    }

    /// No supported backend on this platform: refuse rather than spawn bare.
    #[cfg(not(target_os = "macos"))]
    pub fn spawn(
        &self,
        _command: &mut Command,
        _cwd: &Path,
        _plan: &SandboxPlan,
    ) -> Result<ManagedChild, BrokerError> {
        Err(BrokerError::UnsupportedPlatform)
    }
}
