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

        let prefix = Self::recognised_prefix_of(&canonical).ok_or_else(|| {
            CompileError::ToolchainRootRejected {
                root: canonical.clone(),
                reason: "not inside a recognised toolchain prefix \
                         (/opt/homebrew, /usr/local, /opt/local, /sw, nix, or a system prefix)"
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
