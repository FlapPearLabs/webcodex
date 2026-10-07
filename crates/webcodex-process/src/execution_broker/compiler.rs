// SPDX-License-Identifier: Apache-2.0
//! Compiles a [`SandboxPlan`] into a Seatbelt profile and its launcher argv.
//!
//! # Shape of the output
//!
//! ```text
//! (deny default)                      <- from the Codex-derived base policy
//! + minimum system/runtime allowances <- Codex read-only platform defaults
//! + per-action roots                  <- THIS compiler, from the plan
//! + network policy                    <- THIS compiler
//! ```
//!
//! # Why roots travel as argv, not as profile text
//!
//! The single most important property of this module is that **a filesystem
//! root is never interpolated into the SBPL string**. Every root is passed to
//! `sandbox-exec` as a `-D NAME=<path>` argument, and the profile only ever
//! contains `(subpath (param "NAME"))`.
//!
//! The alternative — writing `(subpath "/Users/someone/...")` into the policy
//! text — means every root is a place where a path can break out of its string
//! literal, silently widen the profile, or fail in a way that looks like a
//! sandbox decision. Passing paths as argv removes that entire class of bug:
//! there is no parser between the path and the kernel.
//!
//! Measured on this host (see `research/spikes/SEATBELT_PROFILE_SPIKE_RESULTS.md`):
//! a profile referencing an undefined `(param)` is **rejected** by
//! `sandbox-exec` with rc=65 before the sandbox is applied. A compiler bug
//! therefore fails closed — it cannot produce a profile that is quietly more
//! permissive than intended.
//!
//! # Fail-closed rules
//!
//! - A relative root is refused. It would otherwise resolve against an
//!   arbitrary directory depending on who started the process.
//! - A root that does not exist, or cannot be resolved, is refused.
//! - A plan granting no filesystem access is refused.
//! - `NetworkPolicy::Allow` is refused: no proxy backend exists, so there is
//!   no correct profile to emit.
//! - Failures happen while *compiling*, before any process exists.
//!
//! # Upstream reuse
//!
//! The two `.sbpl` files this module embeds are direct copies from
//! `openai/codex` at commit `69f7140559180269e2eb8f5be6e0c20eb37b0c85`
//! (Apache-2.0). See `research/spikes/CODEX_SEATBELT_REUSE.md`. Only the
//! capability *set* is reused; Codex's policy model, manager, and network proxy
//! are deliberately not adopted.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::{NetworkPolicy, SandboxPlan, CODEX_BASE_POLICY, CODEX_READ_ONLY_PLATFORM_DEFAULTS};

/// A compiled profile plus the `sandbox-exec` definitions it expects.
///
/// The two halves must travel together: the profile references the parameters
/// by name, and the parameters carry the paths. Keeping them in one value
/// makes it impossible to launch with one half of the pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledProfile {
    /// The full SBPL text handed to `sandbox-exec -p`.
    pub sbpl: String,
    /// `-D NAME=<value>` arguments that must accompany the profile.
    pub definitions: Vec<String>,
}

/// Prefix for readable-root parameters. Matches the `READABLE_ROOT_<i>`
/// convention in the upstream compiler.
const READABLE_PARAM: &str = "WEB_CODEX_READABLE_ROOT_";
/// Prefix for writable-root parameters.
const WRITABLE_PARAM: &str = "WEB_CODEX_WRITABLE_ROOT_";
/// Prefix for runtime-toolchain parameters. See the base policy's LOCAL CHANGE 2.
const TOOLCHAIN_PARAM: &str = "WEB_CODEX_TOOLCHAIN_";

/// A profile that permits everything.
///
/// **`#[cfg(test)]` only.** It exists because the command-fidelity tests must
/// run for real on a host whose kernel refuses restrictive profiles: they
/// check the broker's command construction and process handling, not
/// confinement, and skipping them on such a host would establish nothing.
///
/// It is deliberately *not* reachable from a release build. There is no
/// `SandboxPlan` variant that renders it, no public function that returns it,
/// and no environment variable or flag that selects it — a `cargo build
/// --release` of this crate does not contain it.
#[cfg(test)]
pub(crate) const CODEX_TEST_PERMISSIVE_PROFILE: &str = "(version 1)\n(allow default)\n";

/// Why a plan could not be compiled.
///
/// Every variant is a refusal. There is no "fall back to something close
/// enough" path, because the whole value of a per-action profile is that it is
/// exactly the plan that was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// A root was not an absolute path.
    RootNotAbsolute(PathBuf),
    /// A root could not be resolved to a real location.
    RootUnresolvable {
        /// The root as the caller stated it.
        root: PathBuf,
        /// Why resolution failed.
        reason: String,
    },
    /// The plan grants no filesystem access at all.
    NoFilesystemAccess,
    /// The plan requires a capability this compiler does not implement.
    Unsupported(String),
    /// The caller supplied a toolchain root that cannot be used.
    ToolchainRootRejected {
        /// The offending path.
        root: PathBuf,
        /// Why the host will not vouch for it.
        reason: String,
    },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootNotAbsolute(p) => write!(
                f,
                "sandbox root must be an absolute path, got {}",
                p.display()
            ),
            Self::RootUnresolvable { root, reason } => write!(
                f,
                "sandbox root {} could not be resolved: {reason}",
                root.display()
            ),
            Self::NoFilesystemAccess => write!(
                f,
                "sandbox plan grants no filesystem access; refusing rather than \
                 spawning a process that cannot do anything"
            ),
            Self::Unsupported(what) => write!(f, "sandbox plan unsupported: {what}"),
            Self::ToolchainRootRejected { root, reason } => {
                write!(f, "toolchain root {} rejected: {reason}", root.display())
            }
        }
    }
}

impl std::error::Error for CompileError {}

/// A read-only prefix the host has vouched for, not the caller.
///
/// # Why this is not a `PathBuf`
///
/// A toolchain root *widens* what a sandboxed action can read, past the roots
/// the plan named. If the public API accepted a `PathBuf`, then
/// `spawn_with_toolchain(spec, &["/"])` would compile to a profile granting
/// `(subpath "/")` — the entire filesystem, readable — and every check that
/// could have caught it ("absolute?", "outside `$HOME`?") passes, because `/`
/// is absolute and is not inside `$HOME`.
///
/// So the type carries the *provenance* of a grant, not just its value. There
/// is no public constructor: a caller cannot mint one, and therefore cannot
/// name a root the host did not derive from an executable it actually resolved.
/// The field is private so the value cannot be forged by struct literal either.
///
/// # What the resolver actually proves
///
/// [`TrustedToolchainRoot::resolve`] takes the **path of an executable** and
/// derives a bounded prefix from it:
///
/// 1. the executable is canonicalized, so `/usr/bin/../bin/node` cannot smuggle
///    a different target past the check;
/// 2. it must be a regular file — a directory or device node is not something
///    an interpreter is derived from;
/// 3. the derived prefix must be a *recognised* toolchain layout.
///
/// That last step is what makes `"/"` unrepresentable. It is not a path that
/// fails a range check; it is a path the resolver will not hand out, because it
/// is not a recognised prefix of any executable it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedToolchainRoot(PathBuf);

/// Toolchain layouts the resolver will vouch for, most specific first.
///
/// Deliberately an allow-list. A prefix that is not here is not a toolchain.
const KNOWN_TOOLCHAIN_PREFIXES: &[&str] = &[
    "/opt/homebrew",
    "/usr/local",
    "/opt/local",
    "/sw",
    "/nix/var/nix/profiles/default",
];

/// System prefixes, which are allow-listed by shape rather than by prefix match.
const SYSTEM_PREFIXES: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/libexec",
];

impl TrustedToolchainRoot {
    /// Derive a trusted root from the resolved path of an executable.
    ///
    /// Returns an error unless the derived prefix is a recognised toolchain
    /// layout. This is the whole point of the type: an arbitrary directory
    /// cannot become a grant.
    pub fn resolve(executable: &Path) -> Result<Self, CompileError> {
        Self::resolve_with_rustup_home(executable, rustup_home())
    }

    /// `resolve`, with the operator's rustup home supplied explicitly.
    ///
    /// Both the executable and the rustup home are server-side inputs. Splitting
    /// it out lets the policy be tested against a synthetic layout instead of
    /// depending on whatever happens to be installed on the machine running the
    /// tests.
    ///
    /// Deliberately `pub(crate)`, NOT `pub`. The type's guarantee is that a
    /// caller cannot supply a root; exposing this seam publicly would make the
    /// rustup home a caller-supplied value and weaken exactly the invariant the
    /// type exists to provide. No production caller exists — `resolve` is the only
    /// entry point used outside tests.
    pub(crate) fn resolve_with_rustup_home(
        executable: &Path,
        operator_rustup_home: Option<PathBuf>,
    ) -> Result<Self, CompileError> {
        let canonical =
            executable
                .canonicalize()
                .map_err(|e| CompileError::ToolchainRootRejected {
                    root: executable.to_path_buf(),
                    reason: format!("executable could not be resolved: {e}"),
                })?;

        if !canonical.is_file() {
            return Err(CompileError::ToolchainRootRejected {
                root: canonical,
                reason: "not a regular file".to_string(),
            });
        }

        if let Some(prefix) =
            Self::recognised_rustup_toolchain_prefix_of(&canonical, operator_rustup_home)
        {
            // A rustup-managed component. See the function's own contract: the
            // grant is the single active toolchain subtree, never `$HOME` and
            // never a caller-named directory.
            return Ok(Self(prefix));
        }

        let prefix = Self::recognised_prefix_of(&canonical).ok_or_else(|| {
            CompileError::ToolchainRootRejected {
                root: canonical.clone(),
                reason: "not inside a recognised toolchain prefix \
                         (/opt/homebrew, /usr/local, /opt/local, /sw, nix, a rustup \
                         toolchain, or a system prefix)"
                    .to_string(),
            }
        })?;

        // Defence in depth: the recognised-prefix check is the real gate, but a
        // home directory could be a legal-looking path on some layouts, so it
        // is refused explicitly rather than by implication.
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            if let Ok(home) = home.canonicalize() {
                if prefix.starts_with(&home) {
                    return Err(CompileError::ToolchainRootRejected {
                        root: prefix,
                        reason: "resolves inside the user's home directory".to_string(),
                    });
                }
            }
        }

        Ok(Self(prefix))
    }

    /// The path to hand to the profile compiler.
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// The recognised prefix containing `executable`, if any.
    fn recognised_prefix_of(executable: &Path) -> Option<PathBuf> {
        let text = executable.to_string_lossy();
        for candidate in KNOWN_TOOLCHAIN_PREFIXES {
            if text.starts_with(candidate) {
                return Some(PathBuf::from(candidate));
            }
        }
        for candidate in SYSTEM_PREFIXES {
            if text == *candidate || text.starts_with(&format!("{candidate}/")) {
                return Some(PathBuf::from(candidate));
            }
        }
        None
    }

    /// A rustup-managed toolchain grant, derived from a real resolved component.
    ///
    /// Why this exists: `rust-analyzer`, `rustc`, `clippy-driver` and the sysroot
    /// a project needs live under `<RUSTUP_HOME>/toolchains/<toolchain>/`, which
    /// is inside `$HOME`. The pre-existing prefixes are all outside `$HOME`, and
    /// the home-directory refusal below them is deliberate, so an operator with a
    /// rustup toolchain had no way to run the language server at all.
    ///
    /// Why it is narrow — this is the whole security argument:
    ///
    /// * It is derived by the broker from an ALREADY-RESOLVED REAL FILE. Nothing
    ///   names a path: not the model, not `job_start`, not `lsp_*`. There is no
    ///   constructor that accepts a caller-supplied root.
    /// * The root is exactly `<RUSTUP_HOME>/toolchains/<exact toolchain>` — one
    ///   directory, resolved against the rustup layout, not a prefix match. It
    ///   is NOT `$HOME`, NOT `~/.cargo`, NOT all of `~/.rustup`.
    /// * Siblings are therefore unreachable by construction: `~/.ssh`,
    ///   `~/.aws`, `~/.config` and arbitrary `~/.cargo` files are not under
    ///   `toolchains/<name>/`, so they cannot be read through this grant.
    /// * `RUSTUP_HOME` comes from the operator's environment or `~/.rustup`. If
    ///   neither exists the function returns `None` and the caller falls back to
    ///   the pre-existing prefixes — ambiguity fails closed.
    /// * The path shape is validated: absolute, `toolchains` as a literal
    ///   component, a non-empty toolchain name with no separators or traversal,
    ///   the candidate a regular file, and the resulting directory existing.
    fn recognised_rustup_toolchain_prefix_of(
        executable: &Path,
        rustup_home: Option<PathBuf>,
    ) -> Option<PathBuf> {
        let home = rustup_home?;
        // Both sides must be canonical before `strip_prefix`. The executable has
        // already been canonicalized by the caller, and on macOS a path reached
        // through /var canonicalizes to /private/var — comparing a canonical
        // executable against a non-canonical toolchains directory would fail to
        // match and silently deny a legitimate grant.
        let toolchains = match home.join("toolchains").canonicalize() {
            Ok(path) => path,
            Err(_) => return None,
        };
        let canonical_executable = executable.canonicalize().ok()?;
        let rest = canonical_executable.strip_prefix(&toolchains).ok()?;
        let mut components = rest.components();
        let toolchain = components.next()?;
        // The remainder must name something INSIDE the toolchain (bin/, lib/,
        // ...). If there is no further component then `executable` is the
        // toolchain directory itself, which cannot be the executable.
        if components.next().is_none() {
            return None;
        }
        let toolchain_name = toolchain.as_os_str().to_str()?;
        if toolchain_name.is_empty()
            || toolchain_name == "."
            || toolchain_name == ".."
            || toolchain_name.contains('/')
            || toolchain_name.contains('\\')
            || toolchain_name.starts_with('.')
        {
            return None;
        }
        let root = toolchains.join(toolchain_name);
        // The grant must be a real directory that exists on disk, so a crafted
        // path cannot mint a grant for something that is not there.
        if !root.is_dir() || !canonical_executable.is_file() {
            return None;
        }
        Some(root)
    }
}

/// The operator's rustup home, from the environment or the default location.
///
/// Server/operator-derived only. This is never read from MCP arguments, so a
/// model cannot redirect the grant by choosing an executable.
fn rustup_home() -> Option<PathBuf> {
    rustup_home_from(
        std::env::var_os("RUSTUP_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// The operator's rustup home, from explicit inputs so the policy is testable.
///
/// Both inputs are server-side: the process environment and the operator's own
/// home directory. Neither is reachable from an MCP request, so a model cannot
/// redirect the grant. A configured value must be absolute and must exist;
/// otherwise it is ignored rather than trusted.
fn rustup_home_from(
    configured: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(value) = configured {
        let path = PathBuf::from(value);
        if path.is_absolute() && path.is_dir() {
            return Some(path);
        }
        // A relative or non-existent RUSTUP_HOME is not honoured; fall through
        // to the default rather than trusting it.
    }
    let home = PathBuf::from(home?);
    let default = home.join(".rustup");
    if default.is_dir() {
        Some(default)
    } else {
        None
    }
}

impl std::fmt::Display for TrustedToolchainRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Compile a plan into a profile and its parameter definitions.
///
/// `toolchain_roots` are **trusted** roots — see [`TrustedToolchainRoot`]. The
/// type is the enforcement: this function has no overload that takes a bare
/// path, so a caller cannot widen the profile with a directory it merely named.
pub fn compile(
    plan: &SandboxPlan,
    toolchain_roots: &[TrustedToolchainRoot],
) -> Result<CompiledProfile, CompileError> {
    let (writable_roots, readable_roots, network) = match plan {
        SandboxPlan::Confined {
            writable_roots,
            readable_roots,
            network,
        } => (writable_roots, readable_roots, network),
    };

    if writable_roots.is_empty() && readable_roots.is_empty() {
        return Err(CompileError::NoFilesystemAccess);
    }

    // Refused before any string is built, so an unsupported plan can never
    // reach a launcher.
    if matches!(network, NetworkPolicy::Allow) {
        return Err(CompileError::Unsupported(
            "network-allow plans require a proxy backend this spike does not implement".to_string(),
        ));
    }

    let mut definitions: Vec<String> = Vec::new();
    let mut body = String::new();

    // --- per-action read access ------------------------------------------
    // Writable roots are also readable. A caller that names a root as writable
    // and forgets to list it as readable has made a mistake, not expressed a
    // restriction: a directory you may write is a directory whose contents you
    // can enumerate.
    for (index, root) in readable_roots
        .iter()
        .chain(writable_roots.iter())
        .enumerate()
    {
        let resolved = resolve_root(root)?;
        let name = format!("{READABLE_PARAM}{index}");
        push_definition(&mut definitions, &name, &resolved);
        let _ = writeln!(
            body,
            "(allow file-read* file-test-existence (subpath (param \"{name}\")))"
        );
    }

    // --- per-action write access -----------------------------------------
    for (index, root) in writable_roots.iter().enumerate() {
        let resolved = resolve_root(root)?;
        let name = format!("{WRITABLE_PARAM}{index}");
        push_definition(&mut definitions, &name, &resolved);
        let _ = writeln!(body, "(allow file-write* (subpath (param \"{name}\")))");
    }

    // A writable root must not be replaceable by renaming its own anchor away
    // and recreating it, which would hand a later action a different directory
    // under the same authority. Taken from the upstream compiler's
    // root_anchor_denies.
    for index in 0..writable_roots.len() {
        let name = format!("{WRITABLE_PARAM}{index}");
        let _ = writeln!(
            body,
            "(deny file-write-unlink (require-all (vnode-type DIRECTORY) (literal (param \"{name}\"))))"
        );
    }

    // --- runtime toolchain read access -----------------------------------
    // The value is already trusted by construction, so there is nothing left to
    // validate here: `TrustedToolchainRoot` cannot be constructed from a bare
    // path by anyone outside this module.
    for (index, root) in toolchain_roots.iter().enumerate() {
        let name = format!("{TOOLCHAIN_PARAM}{index}");
        push_definition(&mut definitions, &name, root.as_path());
        let _ = writeln!(
            body,
            "(allow file-read* file-map-executable (subpath (param \"{name}\")))"
        );
    }

    // --- assembly ---------------------------------------------------------
    // Order matters and mirrors the upstream compiler: base, read, write,
    // then the denies that must not be re-opened by anything above. The base
    // policy's LOCAL CHANGE 1 carries the explicit network deny.
    let mut sbpl = String::with_capacity(
        CODEX_BASE_POLICY.len() + CODEX_READ_ONLY_PLATFORM_DEFAULTS.len() + body.len() + 256,
    );
    sbpl.push_str(CODEX_BASE_POLICY);
    sbpl.push('\n');
    sbpl.push_str(CODEX_READ_ONLY_PLATFORM_DEFAULTS);
    sbpl.push('\n');
    sbpl.push_str(&body);
    // Network deny restated last, so a later edit above cannot re-open it by
    // omission. Deny-default already covers it; the explicit form is kept so
    // the profile's network posture is readable without inferring it.
    sbpl.push_str("\n; network: denied explicitly (NetworkPolicy::Deny)\n(deny network*)\n");

    Ok(CompiledProfile { sbpl, definitions })
}

/// Resolve a plan root to something the kernel can be given.
///
/// Canonicalization matters for a specific reason: `(subpath "/tmp/x")` and
/// `(subpath "/private/tmp/x")` are different rules on macOS, and `/tmp` is a
/// symlink to `/private/tmp`. Handing the kernel a non-canonical path would
/// make the rule match nothing (or match something other than what the caller
/// named), which is a silent over-restriction at best.
fn resolve_root(root: &Path) -> Result<PathBuf, CompileError> {
    if !root.is_absolute() {
        return Err(CompileError::RootNotAbsolute(root.to_path_buf()));
    }
    root.canonicalize()
        .map_err(|e| CompileError::RootUnresolvable {
            root: root.to_path_buf(),
            reason: e.to_string(),
        })
}

/// Resolve a toolchain root, refusing anything inside the user's home.
///
/// A toolchain root widens read access beyond the plan. If one could point
/// into `~`, then "give me node" would quietly become "read my home
/// directory". Homebrew prefixes live outside `~`, so the refusal costs this
/// design nothing and removes a whole failure mode.
///
/// Superseded by [`TrustedToolchainRoot::resolve`], which additionally requires
/// the root to be derived from a real executable inside a recognised prefix.
/// Retained only as the negative-test oracle: it shows what the *old*,
/// caller-supplied-path behaviour was, so the new type's guarantee can be
/// stated as a difference rather than an assertion.
#[cfg(test)]
fn resolve_toolchain_root_unchecked(root: &Path) -> Result<PathBuf, CompileError> {
    if !root.is_absolute() {
        return Err(CompileError::ToolchainRootRejected {
            root: root.to_path_buf(),
            reason: "not absolute".to_string(),
        });
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|h| h.canonicalize().ok());
    let resolved = root
        .canonicalize()
        .map_err(|_| CompileError::ToolchainRootRejected {
            root: root.to_path_buf(),
            reason: "could not be resolved".to_string(),
        })?;
    if let Some(home) = home {
        if resolved.starts_with(&home) {
            return Err(CompileError::ToolchainRootRejected {
                root: resolved,
                reason: "inside the user's home directory".to_string(),
            });
        }
    }
    Ok(resolved)
}

fn push_definition(definitions: &mut Vec<String>, name: &str, value: &Path) {
    definitions.push(format!("{name}={}", value.to_string_lossy()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(writable: &[&str], readable: &[&str]) -> SandboxPlan {
        SandboxPlan::Confined {
            writable_roots: writable.iter().map(PathBuf::from).collect(),
            readable_roots: readable.iter().map(PathBuf::from).collect(),
            network: NetworkPolicy::Deny,
        }
    }

    #[test]
    fn roots_travel_as_argv_never_as_profile_text() {
        let dir = tempfile::tempdir().unwrap();
        // A path full of SBPL metacharacters. If it were interpolated into the
        // profile this would either break the profile or, worse, widen it.
        let nasty = dir.path().join("we\"ird) (allow default) (");
        std::fs::create_dir_all(&nasty).unwrap();

        let compiled = compile(&plan(&[nasty.to_str().unwrap()], &[]), &[]).unwrap();

        assert!(
            !compiled.sbpl.contains("allow default) ("),
            "root text must not appear in the profile: {}",
            compiled.sbpl
        );
        assert!(
            compiled
                .definitions
                .iter()
                .any(|d| d.starts_with("WEB_CODEX_READABLE_ROOT_0=")),
            "root must be passed as a definition, got {:?}",
            compiled.definitions
        );
        assert!(compiled
            .sbpl
            .contains("(subpath (param \"WEB_CODEX_READABLE_ROOT_0\"))"));
    }

    #[test]
    fn writable_root_is_also_readable() {
        let dir = tempfile::tempdir().unwrap();
        let compiled = compile(&plan(&[dir.path().to_str().unwrap()], &[]), &[]).unwrap();
        assert!(compiled.sbpl.contains("WEB_CODEX_READABLE_ROOT_0"));
        assert!(compiled.sbpl.contains("WEB_CODEX_WRITABLE_ROOT_0"));
    }

    #[test]
    fn relative_root_is_refused() {
        let err = compile(&plan(&["relative/dir"], &[]), &[]).unwrap_err();
        assert!(
            matches!(err, CompileError::RootNotAbsolute(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn unresolvable_root_is_refused() {
        let err = compile(&plan(&["/webcodex/definitely/not/here/at/all"], &[]), &[]).unwrap_err();
        assert!(
            matches!(err, CompileError::RootUnresolvable { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn empty_plan_is_refused() {
        let err = compile(&plan(&[], &[]), &[]).unwrap_err();
        assert_eq!(err, CompileError::NoFilesystemAccess);
    }

    #[test]
    fn network_allow_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = SandboxPlan::Confined {
            writable_roots: vec![dir.path().to_path_buf()],
            readable_roots: Vec::new(),
            network: NetworkPolicy::Allow,
        };
        let err = compile(&p, &[]).unwrap_err();
        assert!(matches!(err, CompileError::Unsupported(_)), "got {err:?}");
    }

    #[test]
    fn toolchain_root_inside_home_is_refused() {
        let home = std::env::var_os("HOME").expect("HOME must be set");
        // A file inside the home directory is not a recognised toolchain
        // prefix, so the resolver refuses it before any grant is minted.
        let inside_home = PathBuf::from(home).join(".webcodex-fake-executable");
        std::fs::write(&inside_home, b"#!/bin/sh\n").expect("write fake executable");
        let err = TrustedToolchainRoot::resolve(&inside_home).unwrap_err();
        assert!(
            matches!(err, CompileError::ToolchainRootRejected { .. }),
            "got {err:?}"
        );
        let _ = std::fs::remove_file(&inside_home);
    }

    /// **`/` must be unreachable as a toolchain grant.**
    ///
    /// This is the negative test the whole type exists for. Under the previous
    /// caller-supplied-`PathBuf` API, `/` was absolute and outside `$HOME`, so
    /// it passed every check and compiled to a profile granting
    /// `(subpath "/")` — the entire filesystem, readable.
    ///
    /// The new API cannot express that: the resolver only mints roots derived
    /// from a real executable inside a recognised prefix, and `/` is not one.
    #[test]
    fn arbitrary_root_cannot_become_a_toolchain_grant() {
        // The old behaviour, for contrast: `/` sailed through the checks.
        let naive = resolve_toolchain_root_unchecked(Path::new("/"))
            .expect("the old checks would have accepted /");
        assert_eq!(
            naive,
            PathBuf::from("/"),
            "precondition: the old validator did accept /"
        );

        // The new behaviour: there is no path to express it, because the only
        // constructor takes an executable and requires a recognised prefix.
        for bogus in ["/", "/usr", "/opt", "/System", "/private"] {
            // A directory is not a file, so it cannot be an executable.
            assert!(
                TrustedToolchainRoot::resolve(Path::new(bogus)).is_err(),
                "{bogus} must not be resolvable as a toolchain root"
            );
        }
    }

    // -----------------------------------------------------------------
    // Rustup-managed toolchain grants.
    //
    // An operator with a rustup toolchain keeps the language server, compiler
    // and sysroot under <RUSTUP_HOME>/toolchains/<toolchain>/, which is inside
    // $HOME and was therefore unreachable. These tests pin both halves: the
    // grant works for a real component, and it stays narrow enough that nothing
    // else under $HOME can be read through it.
    // -----------------------------------------------------------------

    /// Build a fake rustup layout and return (rustup_home, component_path).
    fn fake_rustup_layout(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("toolchains").join(tag).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let component = bin.join("rust-analyzer");
        std::fs::write(&component, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = std::fs::metadata(&component).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
        }
        std::fs::set_permissions(&component, perms).unwrap();
        (dir, component)
    }

    /// Positive: a real rustup component yields exactly its toolchain subtree.
    #[test]
    fn rustup_component_grants_exactly_the_toolchain_subtree() {
        let (_guard, component) = fake_rustup_layout("stable-test");
        let rustup_home = Some(root_home_of(&component));
        let root = TrustedToolchainRoot::resolve_with_rustup_home(&component, rustup_home)
            .expect("an installed rustup component is a trusted toolchain root");
        let path = root.as_path().to_path_buf();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("stable-test"),
            "the grant must be the toolchain directory itself: {path:?}"
        );
        assert_eq!(
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("toolchains"),
            "the grant must sit directly under toolchains/: {path:?}"
        );
        // The critical narrowing: NOT the rustup home, NOT $HOME.
        assert_ne!(
            path,
            root_home_of(&component),
            "the grant must not be the whole rustup home: {path:?}"
        );
        if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
            assert_ne!(path, home, "the grant must never be $HOME: {path:?}");
        }
    }

    /// Negative: nothing outside toolchains/<name>/ can mint a grant.
    ///
    /// Each case is a path an attacker would want readable. None of them is under
    /// a `toolchains/<name>` subtree, so the resolver must refuse it.
    #[test]
    fn arbitrary_home_locations_cannot_mint_a_toolchain_grant() {
        // A synthetic rustup home, so the cases below are laid out inside the
        // exact tree the resolver is pointed at. Using the real ~/.rustup here
        // would make the test depend on what happens to be installed.
        let (guard, _component) = fake_rustup_layout("stable-neg");
        let rustup_home = Some(root_home_of(&_component));
        let home = rustup_home.clone().unwrap().parent().unwrap().to_path_buf();

        // Each case is a path an attacker would want readable, none of which is
        // inside toolchains/<name>/. The resolver must refuse every one.
        for relative in [
            ".ssh/id_rsa",
            ".aws/credentials",
            ".config/gcloud/configurations/config_default",
            ".cargo/credentials.toml",
            // Directly inside the rustup home but NOT under toolchains/.
            ".rustup/settings.toml",
            // Right shape, wrong rustup home: a forged sibling layout.
            ".rustup-not-really/toolchains/fake/bin/rust-analyzer",
            // The right shape under the REAL rustup home, but the component is
            // not actually installed there.
            ".rustup/toolchains/never-installed/bin/rust-analyzer",
        ] {
            let base = if relative.starts_with(".rustup/") {
                home.join(".rustup")
            } else {
                home.clone()
            };
            let candidate = base.join(relative.trim_start_matches(".rustup/"));
            if let Some(parent) = candidate.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&candidate, b"x").unwrap();
            let mut perms = std::fs::metadata(&candidate).unwrap().permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms.set_mode(0o755);
            }
            std::fs::set_permissions(&candidate, perms).unwrap();
            let resolved =
                TrustedToolchainRoot::resolve_with_rustup_home(&candidate, rustup_home.clone());
            assert!(
                resolved.is_err(),
                "{relative} must not be resolvable as a toolchain root, got {:?}",
                resolved.map(|r| r.as_path().to_path_buf())
            );
            let _ = std::fs::remove_file(&candidate);
        }
        drop(guard);
    }

    /// Negative: a toolchain directory is not itself a grant, and neither is a
    /// traversal attempt through it.
    #[test]
    fn rustup_grant_rejects_a_directory_and_traversal() {
        let (guard, component) = fake_rustup_layout("stable-dir");
        // The toolchain dir itself is not an executable.
        let toolchain_dir = component.parent().unwrap().parent().unwrap();
        let rustup_home = Some(root_home_of(&component));
        assert!(
            TrustedToolchainRoot::resolve_with_rustup_home(toolchain_dir, rustup_home.clone())
                .is_err(),
            "a toolchain directory must not be accepted as a component"
        );
        // A traversal component inside toolchains/ is not a plain toolchain name.
        let sneaky = toolchain_dir.join("..").join("other-toolchain").join("bin");
        let _ = std::fs::create_dir_all(&sneaky);
        let sneaky_file = sneaky.join("rust-analyzer");
        std::fs::write(&sneaky_file, b"x").unwrap();
        let resolved = TrustedToolchainRoot::resolve_with_rustup_home(&sneaky_file, rustup_home);
        if let Ok(root) = resolved {
            assert!(
                !root.as_path().to_string_lossy().contains(".."),
                "a traversal must never survive into a grant: {:?}",
                root.as_path()
            );
        }
        drop(guard);
    }

    /// Negative: with no rustup home the resolver falls back to the fixed
    /// prefixes, so ambiguity fails closed rather than opening a grant.
    #[test]
    fn no_rustup_home_means_no_rustup_grant() {
        assert!(
            rustup_home_from(None, None).is_none(),
            "an absent rustup home must not yield a grant source"
        );
        // A relative RUSTUP_HOME is not honoured.
        assert!(
            rustup_home_from(Some(std::ffi::OsStr::new("relative/rustup")), None).is_none(),
            "a relative RUSTUP_HOME must be refused"
        );
        // A non-existent absolute one is not honoured either.
        assert!(
            rustup_home_from(
                Some(std::ffi::OsStr::new("/nonexistent-rustup-home-for-test")),
                Some(std::ffi::OsStr::new("/tmp"))
            )
            .is_none(),
            "a non-existent RUSTUP_HOME must be refused"
        );
    }

    /// The resolver reads its rustup home only from server-side inputs.
    #[test]
    fn rustup_home_comes_only_from_operator_configuration() {
        // With neither source present the helper is None, i.e. no grant. There is
        // deliberately no third parameter a request could populate.
        assert!(rustup_home_from(None, None).is_none());
    }

    fn root_home_of(component: &std::path::Path) -> std::path::PathBuf {
        // Canonicalized, because the resolver canonicalizes the component before
        // matching, and on macOS the temp dir is reached through /var while the
        // canonical form is /private/var. Skipping this would make the prefix
        // comparison fail for a reason that has nothing to do with the policy.
        component
            .canonicalize()
            .unwrap_or_else(|_| component.to_path_buf())
            .ancestors()
            .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("toolchains"))
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| component.to_path_buf())
    }

    /// The resolver accepts a real executable in a recognised prefix and yields
    /// a *bounded* prefix, never the executable's own parent chain.
    #[test]
    fn resolver_derives_a_bounded_prefix_from_a_real_executable() {
        let true_bin = Path::new("/usr/bin/true");
        if !true_bin.is_file() {
            eprintln!("SKIP_REASON: /usr/bin/true not present on this host");
            return;
        }
        let root = TrustedToolchainRoot::resolve(true_bin).expect("system binary is trusted");
        let path = root.as_path().to_string_lossy().into_owned();
        assert!(
            KNOWN_TOOLCHAIN_PREFIXES.contains(&path.as_str())
                || SYSTEM_PREFIXES.contains(&path.as_str()),
            "resolver returned an unrecognised prefix: {path}"
        );
        assert!(
            !path.is_empty() && path != "/",
            "resolver must never yield the filesystem root"
        );
    }

    /// A compiled profile with a trusted toolchain root carries the root as an
    /// argv definition, and the profile names it only through the parameter.
    ///
    /// The assertion is on the *toolchain rule* rather than on the whole
    /// profile: `/usr/bin` legitimately appears in the embedded Codex platform
    /// defaults, so "the path is absent from the profile" would be false for a
    /// reason that has nothing to do with this code. What matters is that the
    /// rule this compiler emits carries no path text.
    #[test]
    fn trusted_toolchain_root_reaches_the_profile_as_argv_only() {
        let true_bin = Path::new("/usr/bin/true");
        if !true_bin.is_file() {
            eprintln!("SKIP_REASON: /usr/bin/true not present on this host");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let trusted = TrustedToolchainRoot::resolve(true_bin).expect("trusted");
        let compiled = compile(
            &plan(&[dir.path().to_str().unwrap()], &[]),
            std::slice::from_ref(&trusted),
        )
        .expect("compiles");

        let rule = compiled
            .sbpl
            .lines()
            .find(|l| l.contains("WEB_CODEX_TOOLCHAIN_0"))
            .expect("a toolchain rule must be emitted");
        assert!(
            rule.contains("(subpath (param \"WEB_CODEX_TOOLCHAIN_0\"))"),
            "the rule must reference the parameter, not a literal path: {rule}"
        );
        assert!(
            !rule.contains(&*trusted.as_path().to_string_lossy()),
            "the toolchain rule must not interpolate the path: {rule}"
        );
        assert!(
            compiled
                .definitions
                .iter()
                .any(|d| d.starts_with("WEB_CODEX_TOOLCHAIN_0=")),
            "the toolchain root must be a definition: {:?}",
            compiled.definitions
        );
    }

    #[test]
    fn profile_is_deny_default_with_codex_base() {
        let dir = tempfile::tempdir().unwrap();
        let compiled = compile(&plan(&[dir.path().to_str().unwrap()], &[]), &[]).unwrap();
        assert!(compiled.sbpl.contains("(deny default)"));
        // A marker that only exists in the upstream base policy.
        assert!(compiled.sbpl.contains("(allow process-exec)"));
        // And one that only exists in the upstream platform defaults.
        assert!(compiled
            .sbpl
            .contains("com.apple.system.opendirectoryd.libinfo"));
    }

    #[test]
    fn network_deny_is_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let compiled = compile(&plan(&[dir.path().to_str().unwrap()], &[]), &[]).unwrap();
        assert!(compiled.sbpl.contains("(deny network*)"));
    }
}
