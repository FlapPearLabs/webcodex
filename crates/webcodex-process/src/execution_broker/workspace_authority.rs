// SPDX-License-Identifier: Apache-2.0
//! Trusted workspace authority: the only source of a P1 [`SandboxPlan`].
//!
//! # Why this type exists
//!
//! A [`SandboxPlan`] names filesystem roots. Whoever chooses those roots
//! chooses what a model-triggered action can reach. If the *action* chose them,
//! the sandbox would be advisory: a model that asked for `readable_roots:
//! ["/"]` would get a filesystem-wide read grant, and the only thing standing
//! between that and the whole disk is a check somewhere else.
//!
//! So roots are never taken from a tool argument. They come from
//! [`WorkspaceAuthority`], which is built from a **trusted server-side project
//! context** — the Runner's own project registry or its configured
//! `allowed_roots`, never a model-supplied path.
//!
//! # What the model *does* choose
//!
//! The model chooses `command`, `args`, and `cwd`. It does not choose the
//! sandbox root, the toolchain root, or the network posture. `cwd` is not an
//! authority: it is checked *against* the authority by
//! [`ExecutionBroker::check_cwd`](super::ExecutionBroker::check_cwd) and
//! refused when it falls outside, so naming a directory never mints a grant.
//!
//! # P1 static policy
//!
//! Deliberately small, and deliberately not the final product policy:
//!
//! ```text
//! workspace root   READ + WRITE
//! everything else  denied (deny-default profile)
//! network          denied
//! approval         none — this round normalizes execution, not policy
//! ```
//!
//! There is no ASK, no session grant, and no human approval here on purpose.
//! Those belong to a SecurityBroker that does not exist yet; the point of P1
//! is that *when* it is added, no execution path can bypass it.

use std::path::{Path, PathBuf};

use super::compiler::TrustedToolchainRoot;
use super::{NetworkPolicy, SandboxPlan};

/// Why a workspace authority could not be established.
///
/// Every variant is a refusal. There is no fallback to a wider root, because a
/// wider root is exactly the authority this type exists to withhold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityError {
    /// No trusted project context covered the requested directory.
    ///
    /// This is the fail-closed case: rather than guess a root, execution is
    /// refused before any process exists.
    NoTrustedContext {
        /// The directory that was asked about.
        requested: PathBuf,
    },
    /// The trusted root does not resolve to a real directory.
    InvalidTrustedRoot {
        /// The root as trusted configuration stated it.
        root: PathBuf,
        /// Why it could not be used.
        reason: String,
    },
}

impl std::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTrustedContext { requested } => write!(
                f,
                "no trusted project context covers {}; refusing to derive a sandbox \
                 authority rather than guessing one",
                requested.display()
            ),
            Self::InvalidTrustedRoot { root, reason } => write!(
                f,
                "trusted workspace root {} is unusable: {reason}",
                root.display()
            ),
        }
    }
}

impl std::error::Error for AuthorityError {}

/// A workspace root the host vouched for, plus the plan derived from it.
///
/// Construction is the enforcement. There is no public field and no way to
/// build one from a caller-named path: [`WorkspaceAuthority::for_trusted_root`]
/// canonicalizes and validates, and every production caller reaches it through
/// trusted server-side state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAuthority {
    root: PathBuf,
}

impl WorkspaceAuthority {
    /// Establish an authority from a root that trusted configuration supplied.
    ///
    /// `root` must be an absolute path that resolves to a real directory. It is
    /// canonicalized, so a symlinked or `/tmp`-style alias cannot make two
    /// spellings of one directory compare as different authorities.
    pub fn for_trusted_root(root: &Path) -> Result<Self, AuthorityError> {
        if !root.is_absolute() {
            return Err(AuthorityError::InvalidTrustedRoot {
                root: root.to_path_buf(),
                reason: "trusted workspace root must be an absolute path".to_string(),
            });
        }
        let canonical =
            root.canonicalize()
                .map_err(|error| AuthorityError::InvalidTrustedRoot {
                    root: root.to_path_buf(),
                    reason: format!("does not resolve to a real directory: {error}"),
                })?;
        if !canonical.is_dir() {
            return Err(AuthorityError::InvalidTrustedRoot {
                root: canonical,
                reason: "not a directory".to_string(),
            });
        }
        Ok(Self { root: canonical })
    }

    /// Establish the *narrowest* trusted root that contains `candidate`.
    ///
    /// # Narrowest means deepest, not first
    ///
    /// This used to return the first entry in `trusted_roots` that covered
    /// `candidate`, on the stated assumption that callers ordered their roots
    /// most-specific first. That made the result a property of the caller's
    /// ordering rather than of the filesystem: a caller that passed a coarse
    /// `$HOME` root before a project root would confine the project to the whole
    /// home directory, and the "narrowest-wins" guarantee held only as long as
    /// every caller got the order right.
    ///
    /// It now measures. Every root that covers `candidate` is canonicalized and
    /// the deepest one wins, so `/tmp/root` and `/tmp/root/project` resolve to
    /// the project in either order. Dependence on caller ordering is not a
    /// property a security boundary can have.
    ///
    /// Returns [`AuthorityError::NoTrustedContext`] when nothing covers
    /// `candidate`. Callers must refuse execution on that error rather than
    /// falling back to a broader root.
    pub fn narrowest_covering(
        candidate: &Path,
        trusted_roots: &[PathBuf],
    ) -> Result<Self, AuthorityError> {
        let canonical =
            candidate
                .canonicalize()
                .map_err(|error| AuthorityError::InvalidTrustedRoot {
                    root: candidate.to_path_buf(),
                    reason: format!("does not resolve to a real directory: {error}"),
                })?;
        let mut narrowest: Option<Self> = None;
        for root in trusted_roots {
            // An unusable trusted root is skipped rather than fatal: another
            // entry may still cover the candidate, and the *absence* of any
            // cover is what fails closed.
            let Ok(authority) = Self::for_trusted_root(root) else {
                continue;
            };
            if !(canonical == authority.root || canonical.starts_with(&authority.root)) {
                continue;
            }
            // Deepest wins. `components().count()` is the depth measure that
            // works for both absolute and relative canonical paths, and it
            // cannot be fooled by a longer string: containment was already
            // checked component-wise above.
            let is_narrower = narrowest
                .as_ref()
                .is_none_or(|current| depth(&authority.root) > depth(&current.root));
            if is_narrower {
                narrowest = Some(authority);
            }
        }
        narrowest.ok_or(AuthorityError::NoTrustedContext {
            requested: candidate.to_path_buf(),
        })
    }

    /// The canonical workspace root this authority grants.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The P1 static policy as a [`SandboxPlan`].
    ///
    /// Workspace read+write, nothing else readable, network denied. The plan is
    /// a value derived entirely from trusted state: no field here can be
    /// influenced by a tool argument.
    pub fn plan(&self) -> SandboxPlan {
        SandboxPlan::Confined {
            writable_roots: vec![self.root.clone()],
            // Writable roots are implicitly readable in the compiler, so an
            // empty readable list is a *narrower* grant, not a wider one.
            readable_roots: Vec::new(),
            network: NetworkPolicy::Deny,
        }
    }

    /// Read-only toolchain prefixes needed to *start* `program`.
    ///
    /// A deny-default profile must be able to execute the interpreter the
    /// action asked for. The grant is derived by the host from an executable it
    /// actually resolved, inside a recognised toolchain prefix — never from a
    /// caller-supplied directory. See [`TrustedToolchainRoot`].
    ///
    /// Returns an empty vector when no extra grant is needed or possible. That
    /// is not a fallback to a wider plan: the deny-default profile still
    /// applies, so an unresolvable interpreter simply fails to start.
    pub fn toolchain_roots_for(&self, program: &Path) -> Vec<TrustedToolchainRoot> {
        // A program inside the workspace needs no extra read grant.
        let Ok(canonical) = program.canonicalize() else {
            return Vec::new();
        };
        if canonical == self.root || canonical.starts_with(&self.root) {
            return Vec::new();
        }
        TrustedToolchainRoot::resolve(&canonical)
            .into_iter()
            .collect()
    }
}

/// How many path components a canonical root has.
///
/// Used to pick the deepest covering root. Component count rather than string
/// length, because a longer *name* does not mean a deeper path: `/a/long-name`
/// and `/a/b` are both depth 2, and comparing lengths would order them by
/// spelling.
fn depth(path: &Path) -> usize {
    path.components().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn plan_grants_the_workspace_and_nothing_else() {
        let workspace = temp_dir();
        let authority = WorkspaceAuthority::for_trusted_root(workspace.path()).unwrap();
        let SandboxPlan::Confined {
            writable_roots,
            readable_roots,
            network,
        } = authority.plan();
        assert_eq!(writable_roots.len(), 1);
        assert!(readable_roots.is_empty(), "P1 grants no extra read roots");
        assert_eq!(network, NetworkPolicy::Deny);
        assert_eq!(writable_roots[0], authority.root());
    }

    #[test]
    fn relative_trusted_root_is_refused() {
        let err = WorkspaceAuthority::for_trusted_root(Path::new("relative/dir")).unwrap_err();
        assert!(matches!(err, AuthorityError::InvalidTrustedRoot { .. }));
    }

    #[test]
    fn unresolvable_trusted_root_is_refused() {
        let err = WorkspaceAuthority::for_trusted_root(Path::new("/webcodex/definitely/not/here"))
            .unwrap_err();
        assert!(matches!(err, AuthorityError::InvalidTrustedRoot { .. }));
    }

    #[test]
    fn a_file_is_not_a_workspace() {
        let dir = temp_dir();
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let err = WorkspaceAuthority::for_trusted_root(&file).unwrap_err();
        assert!(matches!(err, AuthorityError::InvalidTrustedRoot { .. }));
    }

    /// The narrowest covering root wins, so a project inside `$HOME` confines
    /// to the project rather than to the whole home directory — **in either
    /// order**.
    ///
    /// The second half used to assert the opposite: with the home root listed
    /// first, it asserted that home won. That assertion documented the old
    /// first-match behaviour as if it were intended, which is exactly how an
    /// ordering-dependent security property survives a refactor: it is written
    /// down as a test. Narrowest has to mean narrowest regardless of what order
    /// the caller happened to use.
    #[test]
    fn narrowest_covering_prefers_the_project_over_the_home_root() {
        let home = temp_dir();
        let project = home.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        let inside = project.join("src");

        for roots in [
            vec![project.clone(), home.path().to_path_buf()],
            vec![home.path().to_path_buf(), project.clone()],
        ] {
            let authority = WorkspaceAuthority::narrowest_covering(&inside, &roots)
                .expect("the project covers the candidate");
            assert_eq!(
                authority.root(),
                project.canonicalize().unwrap(),
                "with roots {roots:?} the authority must be the project, never the home root"
            );
        }
    }

    #[test]
    fn no_trusted_context_fails_closed() {
        let dir = temp_dir();
        let outside = temp_dir();
        let err =
            WorkspaceAuthority::narrowest_covering(outside.path(), &[dir.path().to_path_buf()])
                .unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    #[test]
    fn empty_trusted_roots_fail_closed() {
        let dir = temp_dir();
        let err = WorkspaceAuthority::narrowest_covering(dir.path(), &[]).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    #[test]
    fn toolchain_grant_never_covers_the_workspace_or_the_filesystem_root() {
        let workspace = temp_dir();
        let authority = WorkspaceAuthority::for_trusted_root(workspace.path()).unwrap();

        // A program inside the workspace needs no additional grant.
        let inside = workspace.path().join("tool.sh");
        std::fs::write(&inside, b"#!/bin/sh\n").unwrap();
        assert!(authority.toolchain_roots_for(&inside).is_empty());

        // `/` is never handed out: it is not a recognised toolchain layout.
        if Path::new("/usr/bin/true").is_file() {
            for grant in authority.toolchain_roots_for(Path::new("/usr/bin/true")) {
                assert_ne!(grant.as_path(), Path::new("/"));
            }
        }
    }
}
