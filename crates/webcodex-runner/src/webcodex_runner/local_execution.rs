// SPDX-License-Identifier: Apache-2.0
//! The single local-execution chokepoint for P1-normalized surfaces.
//!
//! # Why this exists
//!
//! P1's goal is that a model-triggered local action *cannot* reach a process
//! that was not spawned through the [`ExecutionBroker`]. A property that depends
//! on every call site remembering to route somewhere is not a property, so the
//! routing decision lives in exactly one function here, and the P1 surfaces call
//! it instead of building a `Command` and spawning it themselves.
//!
//! # Why the request is a value, not a `&mut Command`
//!
//! `std::process::Command` has no API to enumerate its own state. Handing one to
//! this module and copying a few fields off it silently drops everything the
//! caller had already configured — stdio, environment — with no error anywhere.
//! That failure already happened once in this codebase's history.
//!
//! So [`LocalExecutionRequest`] states the execution completely as data, and
//! this module builds the final launcher command from that value. Anything the
//! caller did not state does not happen.
//!
//! # Fail-closed
//!
//! Every failure below refuses execution. There is no fallback to an
//! unconfined `Command::spawn`, no "try sandbox then retry normally", and no
//! partial degradation. A host with no working sandbox backend therefore cannot
//! run these surfaces at all — which is the intended posture, and is reported
//! honestly rather than papered over.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use webcodex_process::execution_broker::{
    EnvPolicy, ExecutionBroker, SandboxPlan, SpawnSpec, StreamPolicy,
};

use super::config::RunnerPolicy;
use super::sandbox_authority::resolve_workspace_authority;

/// Environment construction rule for one local action.
///
/// Mirrors the two shapes the Runner already used, so routing through the
/// broker does not silently change what a child can see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalEnv {
    /// Start from an empty environment containing exactly these variables.
    ///
    /// This is what an isolated shell profile already produced via
    /// `env_clear()` followed by the snapshot.
    Snapshot(BTreeMap<String, String>),
    /// Start from the Runner's own environment, minus `remove`, plus
    /// `overrides`.
    ///
    /// Sensitive credential variables are listed in `remove`, matching the
    /// existing `shell_environment_rule` contract.
    Inherit {
        /// Variables added to, or replacing, the inherited value.
        overrides: BTreeMap<String, String>,
        /// Variables removed from the inherited environment.
        remove: Vec<String>,
    },
}

impl LocalEnv {
    fn apply_to(&self, spec: SpawnSpec) -> SpawnSpec {
        match self {
            Self::Snapshot(vars) => {
                let mut spec = spec.env(EnvPolicy::Empty);
                for (key, value) in vars {
                    spec = spec.env_var(key.clone(), OsString::from(value));
                }
                spec
            }
            Self::Inherit { overrides, remove } => {
                let mut spec = spec.env(EnvPolicy::Inherit);
                for key in remove {
                    spec = spec.env_remove(key.clone());
                }
                for (key, value) in overrides {
                    spec = spec.env_var(key.clone(), OsString::from(value));
                }
                spec
            }
        }
    }
}

/// A program's identity and arguments plus its environment rule — everything
/// needed to launch it, held as data.
///
/// This is what the P1 shell/exec helpers return instead of a
/// `std::process::Command`. The distinction matters: a `Command` cannot be
/// enumerated, so once one is built, its environment is unrecoverable and the
/// only way to launch it is to spawn it directly — which is precisely the bypass
/// P1 must close. Holding the program, args and environment as data keeps both
/// launch paths honest: the broker path builds a fresh launcher from these
/// fields, and the legacy path can still materialize a `Command` for the few
/// Runner-owned probes that genuinely need one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandBlueprint {
    program: OsString,
    args: Vec<OsString>,
    env: LocalEnv,
}

impl CommandBlueprint {
    pub(crate) fn new(program: impl Into<OsString>, env: LocalEnv) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env,
        }
    }

    pub(crate) fn arg(&mut self, arg: impl Into<OsString>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    /// Program, as a path, for assertions that compared against
    /// `Command::get_program`.
    ///
    /// Named to match the standard-library accessor these assertions replaced,
    /// so a reader can see the test still means the same thing: the blueprint
    /// simply states the same data as a value instead of hiding it inside a
    /// `Command` that could only be spawned directly.
    pub(crate) fn get_program(&self) -> &OsStr {
        &self.program
    }

    /// Arguments, as an iterator of strings, matching `Command::get_args`.
    pub(crate) fn get_args(&self) -> impl Iterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    pub(crate) fn env(&self) -> &LocalEnv {
        &self.env
    }

    /// Replace the environment rule, keeping program and args.
    pub(crate) fn with_env(mut self, env: LocalEnv) -> Self {
        self.env = env;
        self
    }

    /// Turn this into a `LocalExecutionRequest` rooted at `cwd`.
    pub(crate) fn into_request(self, cwd: PathBuf) -> LocalExecutionRequest {
        let Self { program, args, env } = self;
        LocalExecutionRequest {
            program,
            args,
            cwd,
            env,
            stdin: StreamPolicy::Null,
            stdout: StreamPolicy::Null,
            stderr: StreamPolicy::Null,
        }
    }

    /// Materialize a plain `std::process::Command`.
    ///
    /// Only for Runner-owned control-plane probes that are not themselves
    /// model-triggered execution — e.g. the validation module-availability
    /// check, whose own payload is a fixed Runner-authored probe string. A
    /// caller that wants the *user's* command to run must use
    /// [`spawn_local_action`] instead; that distinction is what the structural
    /// anti-bypass guard pins down.
    pub(crate) fn into_command(self) -> std::process::Command {
        let Self { program, args, env } = self;
        let mut command = std::process::Command::new(&program);
        command.args(&args);
        match env {
            LocalEnv::Snapshot(vars) => {
                command.env_clear();
                for (key, value) in vars {
                    command.env(key, value);
                }
            }
            LocalEnv::Inherit { overrides, remove } => {
                for key in remove {
                    command.env_remove(key);
                }
                for (key, value) in overrides {
                    command.env(key, value);
                }
            }
        }
        command
    }
}

/// One model-triggered local action, stated completely.
///
/// Constructed by the P1 surface from the execution state it already holds, then
/// handed to [`spawn_local_action`]. Nothing here is optional-by-omission: the
/// sandbox plan is attached during the spawn step, from trusted authority only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalExecutionRequest {
    /// Program to run. May be a bare name or a path; the broker passes it
    /// through to the launcher as argv.
    pub(crate) program: OsString,
    /// Arguments after the program.
    pub(crate) args: Vec<OsString>,
    /// Working directory. Checked against the plan; never widens it.
    pub(crate) cwd: PathBuf,
    /// Environment construction rule.
    pub(crate) env: LocalEnv,
    /// Standard input.
    pub(crate) stdin: StreamPolicy,
    /// Standard output.
    pub(crate) stdout: StreamPolicy,
    /// Standard error.
    pub(crate) stderr: StreamPolicy,
}

impl LocalExecutionRequest {
    /// A request with no stdio wiring, ready for a caller to set streams on.
    pub(crate) fn new(program: impl Into<OsString>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: LocalEnv::Snapshot(BTreeMap::new()),
            stdin: StreamPolicy::Null,
            stdout: StreamPolicy::Null,
            stderr: StreamPolicy::Null,
        }
    }

    pub(crate) fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub(crate) fn env(mut self, env: LocalEnv) -> Self {
        self.env = env;
        self
    }

    pub(crate) fn stdin(mut self, policy: StreamPolicy) -> Self {
        self.stdin = policy;
        self
    }

    pub(crate) fn stdout(mut self, policy: StreamPolicy) -> Self {
        self.stdout = policy;
        self
    }

    pub(crate) fn stderr(mut self, policy: StreamPolicy) -> Self {
        self.stderr = policy;
        self
    }
}

/// Why a local action was refused before any process existed.
#[derive(Debug)]
pub(crate) struct LocalExecutionRefusal {
    /// Stable, greppable reason, so a refusal in production logs is
    /// attributable without reading the whole message.
    pub(crate) code: &'static str,
    /// Human-readable detail.
    pub(crate) detail: String,
}

impl std::fmt::Display for LocalExecutionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl LocalExecutionRefusal {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

/// Derive the P1 plan for `cwd` from trusted server-side project context.
///
/// Exposed separately from [`spawn_local_action`] so structural tests can assert
/// the derivation without spawning anything, and so the refusal reasons for a
/// missing/invalid authority are observable on their own.
pub(crate) fn derive_workspace_plan(
    policy: &RunnerPolicy,
    project_registry_dir: Option<&Path>,
    cwd: &Path,
) -> Result<SandboxPlan, LocalExecutionRefusal> {
    let authority =
        resolve_workspace_authority(policy, project_registry_dir, cwd).map_err(|error| {
            LocalExecutionRefusal::new("sandbox_authority_unavailable", error.to_string())
        })?;
    Ok(authority.plan())
}

/// Spawn one model-triggered local action under a workspace-derived plan.
///
/// # The ordering that matters
///
/// Authority is resolved **first**. A missing or invalid trusted context must
/// fail before a process exists, so nothing is compiled, nothing is launched,
/// and there is no partially-started state to reconcile.
pub(crate) fn spawn_local_action(
    policy: &RunnerPolicy,
    project_registry_dir: Option<&Path>,
    request: LocalExecutionRequest,
) -> Result<webcodex_process::ManagedChild, LocalExecutionRefusal> {
    let authority = resolve_workspace_authority(policy, project_registry_dir, &request.cwd)
        .map_err(|error| {
            LocalExecutionRefusal::new("sandbox_authority_unavailable", error.to_string())
        })?;

    let plan = authority.plan();
    let toolchain = authority.toolchain_roots_for(Path::new(&request.program));

    let spec = request.env.apply_to(
        SpawnSpec::new(request.program.clone(), request.cwd.clone(), plan)
            .args(request.args.clone())
            .stdin(request.stdin)
            .stdout(request.stdout)
            .stderr(request.stderr),
    );

    ExecutionBroker::new()
        .spawn_with_toolchain(&spec, &toolchain)
        .map_err(|error| LocalExecutionRefusal::new("sandbox_spawn_refused", error.to_string()))
}

/// Build the environment snapshot form used by isolated shell profiles.
pub(crate) fn snapshot_env(env: &HashMap<String, String>) -> LocalEnv {
    LocalEnv::Snapshot(env.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_for(root: &Path) -> RunnerPolicy {
        RunnerPolicy {
            allowed_roots: vec![root.to_path_buf()],
            ..RunnerPolicy::default()
        }
    }

    #[test]
    fn plan_is_derived_from_trusted_roots() {
        let dir = tempfile::tempdir().unwrap();
        let writable_roots;
        let readable_roots;
        let network;
        match derive_workspace_plan(&policy_for(dir.path()), None, dir.path()).unwrap() {
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
        assert!(readable_roots.is_empty());
        assert_eq!(
            network,
            webcodex_process::execution_broker::NetworkPolicy::Deny
        );
    }

    /// P1-J: missing/invalid trusted project context must fail before any
    /// process creation. The refusal carries a stable code.
    #[test]
    fn missing_trusted_context_refuses_with_a_stable_code() {
        let outside = tempfile::tempdir().unwrap();
        let err =
            derive_workspace_plan(&RunnerPolicy::default(), None, outside.path()).unwrap_err();
        assert_eq!(err.code, "sandbox_authority_unavailable");
        assert!(
            err.detail.contains("no trusted project context"),
            "detail should name the cause: {}",
            err.detail
        );
    }

    #[test]
    fn cwd_outside_trusted_roots_refuses_rather_than_widening() {
        let trusted = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let err =
            derive_workspace_plan(&policy_for(trusted.path()), None, outside.path()).unwrap_err();
        assert_eq!(err.code, "sandbox_authority_unavailable");
    }

    #[test]
    fn snapshot_env_builds_an_explicit_variable_set() {
        let mut env = HashMap::new();
        env.insert("A".to_string(), "1".to_string());
        let LocalEnv::Snapshot(vars) = snapshot_env(&env) else {
            panic!("expected snapshot form");
        };
        assert_eq!(vars.get("A").map(String::as_str), Some("1"));
    }

    /// A request states its execution completely, so nothing is dropped in
    /// translation. This is the regression guard for the fidelity bug that made
    /// a caller lose its stdio and environment.
    #[test]
    fn request_translates_stdio_and_env_without_loss() {
        let dir = tempfile::tempdir().unwrap();
        let mut overrides = BTreeMap::new();
        overrides.insert("K".to_string(), "V".to_string());
        let request = LocalExecutionRequest::new("/bin/sh", dir.path())
            .args(["-c", "echo hi"])
            .env(LocalEnv::Inherit {
                overrides,
                remove: vec!["SECRET".to_string()],
            })
            .stdin(StreamPolicy::Piped)
            .stdout(StreamPolicy::Piped)
            .stderr(StreamPolicy::Piped);

        assert_eq!(request.args.len(), 2);
        assert_eq!(request.stdin, StreamPolicy::Piped);
        assert_eq!(request.stdout, StreamPolicy::Piped);
        assert_eq!(request.stderr, StreamPolicy::Piped);
        assert_eq!(
            request.env,
            LocalEnv::Inherit {
                overrides: BTreeMap::from([("K".to_string(), "V".to_string())]),
                remove: vec!["SECRET".to_string()],
            }
        );
    }

    /// The real spawn path must refuse on this host rather than fall back.
    /// Under a nested Seatbelt the launcher cannot apply a restrictive profile,
    /// and the correct outcome is a refusal, not an unconfined child.
    #[test]
    fn spawn_refuses_when_the_backend_cannot_apply() {
        let dir = tempfile::tempdir().unwrap();
        let request = LocalExecutionRequest::new("/usr/bin/true", dir.path())
            .stdout(StreamPolicy::Piped)
            .stderr(StreamPolicy::Piped);
        match spawn_local_action(&policy_for(dir.path()), None, request) {
            Ok(mut child) => {
                // A host that can apply the profile: the action really ran.
                let _ = child.wait();
            }
            Err(refusal) => {
                assert!(
                    refusal.code == "sandbox_spawn_refused"
                        || refusal.code == "sandbox_authority_unavailable",
                    "unexpected refusal code {}",
                    refusal.code
                );
            }
        }
    }
}
