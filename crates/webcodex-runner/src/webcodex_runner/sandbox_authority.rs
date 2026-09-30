// SPDX-License-Identifier: Apache-2.0
//! Runner-side resolution of the trusted workspace authority for P1.
//!
//! # The rule this file enforces
//!
//! A P1 sandbox plan's roots come from **trusted server-side state only**: the
//! Runner's own project registry and its configured `allowed_roots`. They never
//! come from a model-supplied `cwd`, a tool argument, or anything the request
//! body carried.
//!
//! `cwd` is treated as a *request*, not as authority. It is canonicalized and
//! matched against the trusted roots; if no trusted root contains it, execution
//! is refused before any process exists. That ordering is the whole security
//! property: a model cannot widen its sandbox by naming a wider directory,
//! because naming one simply fails the lookup.
//!
//! # Narrowest-wins
//!
//! The project registry is consulted before `allowed_roots`. A project nested
//! inside `$HOME` therefore confines to the project, not to the entire home
//! directory. Coarse roots are a fallback for "the user said any cwd under
//! here", not a default.

use std::path::{Path, PathBuf};

use webcodex_process::execution_broker::{AuthorityError, WorkspaceAuthority};

use super::config::RunnerPolicy;

/// Resolve the trusted authority for `requested_cwd`.
///
/// # Trust order
///
/// 1. The Runner project registry — the project that contains `requested_cwd`,
///    deepest first. This is the narrowest correct answer for a real project.
/// 2. `policy.allowed_roots` — configured by the host operator, so trusted, but
///    coarse. Only used when no registered project covers the directory.
///
/// # Fail-closed
///
/// Returns [`AuthorityError::NoTrustedContext`] when neither covers the
/// directory. Callers must treat that as "refuse execution". There is
/// deliberately no fallback to the process's own cwd, to `/`, or to an
/// unconfined spawn.
pub(crate) fn resolve_workspace_authority(
    policy: &RunnerPolicy,
    project_registry_dir: Option<&Path>,
    requested_cwd: &Path,
) -> Result<WorkspaceAuthority, AuthorityError> {
    let mut trusted: Vec<PathBuf> = Vec::new();

    if let Some(registry_dir) = project_registry_dir {
        // `find_project_shell_context` already returns the *deepest* project
        // containing the cwd, which is exactly the narrowest-wins rule.
        if let Some(project) =
            super::projects::find_project_shell_context(registry_dir, requested_cwd)
        {
            trusted.push(PathBuf::from(project.path));
        }
    }

    trusted.extend(policy.allowed_roots.iter().cloned());

    WorkspaceAuthority::narrowest_covering(requested_cwd, &trusted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_roots_alone_can_establish_authority() {
        let dir = tempfile::tempdir().unwrap();
        let policy = RunnerPolicy {
            allowed_roots: vec![dir.path().to_path_buf()],
            ..RunnerPolicy::default()
        };
        let authority = resolve_workspace_authority(&policy, None, dir.path())
            .expect("covered by allowed_roots");
        assert_eq!(authority.root(), dir.path().canonicalize().unwrap());
    }

    /// The core fail-closed case: no trusted root covers the directory, so no
    /// plan is derived and the caller must refuse.
    #[test]
    fn uncovered_cwd_fails_closed() {
        let trusted = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let policy = RunnerPolicy {
            allowed_roots: vec![trusted.path().to_path_buf()],
            ..RunnerPolicy::default()
        };
        let err = resolve_workspace_authority(&policy, None, outside.path()).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    #[test]
    fn empty_policy_roots_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let policy = RunnerPolicy::default();
        let err = resolve_workspace_authority(&policy, None, dir.path()).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    /// A cwd *under* a trusted root is covered; a sibling with a shared string
    /// prefix is not. This is the component-wise containment property that
    /// stops `/repo-evil` from inheriting `/repo`'s authority.
    #[test]
    fn containment_is_component_wise_not_string_prefix() {
        let root = tempfile::tempdir().unwrap();
        let inside = root.path().join("src");
        std::fs::create_dir_all(&inside).unwrap();
        let sibling = root.path().to_string_lossy().into_owned() + "-evil";
        std::fs::create_dir_all(&sibling).unwrap();

        let policy = RunnerPolicy {
            allowed_roots: vec![root.path().to_path_buf()],
            ..RunnerPolicy::default()
        };
        assert!(resolve_workspace_authority(&policy, None, &inside).is_ok());

        let sibling_path = PathBuf::from(&sibling);
        let err = resolve_workspace_authority(&policy, None, &sibling_path).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }
}
