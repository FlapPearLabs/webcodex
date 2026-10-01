// SPDX-License-Identifier: Apache-2.0
//! Runner-side resolution of the trusted workspace authority for P1.
//!
//! # The rule this file enforces
//!
//! A P1 sandbox plan's root comes from **one** source: the Runner's own project
//! registry. It never comes from a model-supplied `cwd`, a tool argument, or the
//! request body — and, as of the P1 closure, it never comes from
//! `policy.allowed_roots` either.
//!
//! `cwd` is treated as a *request*, not as authority. It is canonicalized and
//! matched against the registered projects; if no registered project contains
//! it, execution is refused before any process exists. That ordering is the
//! whole security property: a model cannot widen its sandbox by naming a wider
//! directory, because naming one simply fails the lookup.
//!
//! # Why `allowed_roots` is not authority (P1 closure)
//!
//! `RunnerPolicy::allowed_roots` looks like the operator's answer to "where may
//! work happen", and for *file operations* it still is. It is not an execution
//! authority, for a reason that is about provenance rather than about taste:
//!
//! `load_config` runs every configured policy through
//! `effective_allowed_roots`, which **replaces an empty list with `[$HOME]`**.
//! By the time any runtime code reads `policy.allowed_roots`, the difference
//! between
//!
//! * an operator who deliberately wrote `allowed_roots = ["/Users/x/work"]`, and
//! * an operator who wrote nothing at all and silently received their own home
//!   directory
//!
//! has been erased — both are just a `Vec<PathBuf>` with one home-shaped entry.
//! There is no flag, no separate field, and no config version that preserves it.
//!
//! Inferring the distinction anyway would mean guessing, and guessing here fails
//! in the unsafe direction: `$HOME` as an execution authority hands every
//! model-triggered action the user's entire home directory — `~/.ssh`,
//! `~/.aws`, `~/.config`, every credential and every source tree they own. A
//! single `.gitconfig` or `~/.netrc` read is a full credential compromise, and
//! the sandbox that was supposed to prevent exactly that would be the thing that
//! handed it over.
//!
//! So P1 does not infer it. **P1 accepts registered project context and nothing
//! else.** A project the operator registered is an explicit, named, durable
//! grant to one directory; `$HOME` is neither.
//!
//! This is a deliberate compatibility change and it fails **closed**: a
//! deployment that today runs shell commands against a directory merely covered
//! by `allowed_roots` — with no registered project — will start receiving
//! `sandbox_authority_unavailable` until that directory is registered as a
//! project. Registering it is one `webcodex project add`, it makes the grant
//! explicit, and it is the grant the operator meant anyway. See
//! `research/implementation/NORMALIZATION_P1_REPORT.md`.
//!
//! # Narrowest-wins still applies
//!
//! The registry lookup returns the *deepest* project containing the cwd, and the
//! authority is that project's root. Nested projects therefore confine to the
//! inner one, and the result does not depend on the order roots were supplied
//! in.

use std::path::{Path, PathBuf};

use webcodex_process::execution_broker::{AuthorityError, WorkspaceAuthority};

/// Resolve the trusted authority for `requested_cwd`.
///
/// # The only accepted answer
///
/// A registered project whose root contains `requested_cwd`. Deepest such
/// project wins, which is the narrowest correct answer.
///
/// # Fail-closed
///
/// Returns [`AuthorityError::NoTrustedContext`] when no registered project
/// covers the directory. Callers must treat that as "refuse execution". There
/// is deliberately no fallback to `policy.allowed_roots`, to the process's own
/// cwd, to `$HOME`, to `/`, or to an unconfined spawn.
///
/// Note the signature: there is no `policy` parameter. That is the enforcement.
/// Once this function could read `policy.allowed_roots`, re-introducing the
/// `$HOME` fallback would be a one-line change that no compiler would flag and
/// no reviewer would necessarily recognise, because it would look like adding a
/// second trusted source rather than restoring a hole. Removing the parameter
/// makes the property structural instead of documentary.
pub(crate) fn resolve_workspace_authority(
    project_registry_dir: Option<&Path>,
    requested_cwd: &Path,
) -> Result<WorkspaceAuthority, AuthorityError> {
    let Some(registry_dir) = project_registry_dir else {
        // No registry means no registered project, which means no authority.
        return Err(AuthorityError::NoTrustedContext {
            requested: requested_cwd.to_path_buf(),
        });
    };

    // `find_project_shell_context` already returns the *deepest* project
    // containing the cwd, which is exactly the narrowest-wins rule.
    let Some(project) = super::projects::find_project_shell_context(registry_dir, requested_cwd)
    else {
        return Err(AuthorityError::NoTrustedContext {
            requested: requested_cwd.to_path_buf(),
        });
    };

    // Only the project's own root is offered, and only that one entry, so the
    // depth comparison inside `narrowest_covering` has nothing coarse to pick.
    //
    // The call is kept rather than inlined for its canonicalization and
    // directory validation: `for_trusted_root` is the only place that rejects a
    // non-absolute or non-directory root. Note that a registry entry whose
    // directory has since been deleted is already dropped upstream by
    // `find_project_shell_context` (it canonicalizes each candidate and skips
    // the ones that no longer resolve), so a stale entry surfaces as
    // `NoTrustedContext` rather than `InvalidTrustedRoot` — still a refusal,
    // just attributed to "no registered project covers this" instead.
    WorkspaceAuthority::narrowest_covering(requested_cwd, &[PathBuf::from(project.path)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a registry entry in the exact on-disk format production reads:
    /// one `<name>.toml` per project with `id` and `path`.
    fn write_registry(registry: &Path, id: &str, project_path: &Path) {
        std::fs::create_dir_all(registry).unwrap();
        std::fs::write(
            registry.join(format!("{id}.toml")),
            format!(
                "id = \"{id}\"\npath = {:?}\n",
                project_path.to_string_lossy()
            ),
        )
        .unwrap();
    }

    /// The closure's core case: `$HOME` present as a *file-operation* root does
    /// not create an execution authority. There is no `policy` to pass any more,
    /// which is the point — the parameter that carried `$HOME` is gone.
    #[test]
    fn home_is_not_an_execution_authority() {
        let home = tempfile::tempdir().unwrap();
        let loose = home.path().join("not-a-project");
        std::fs::create_dir_all(&loose).unwrap();
        // An empty registry directory: the operator has projects configured,
        // none of which covers this directory.
        let registry = home.path().join("registry");
        std::fs::create_dir_all(&registry).unwrap();

        let err = resolve_workspace_authority(Some(&registry), &loose).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    /// A registered project is authority, and the authority is the *project*
    /// root — never the enclosing home directory.
    #[test]
    fn registered_project_under_home_yields_the_project_root() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        let src = project.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let registry = home.path().join("registry");
        write_registry(&registry, "closure-project", &project);

        let authority =
            resolve_workspace_authority(Some(&registry), &src).expect("registered project");
        assert_eq!(authority.root(), project.canonicalize().unwrap());
        assert_ne!(
            authority.root(),
            home.path().canonicalize().unwrap(),
            "authority must be the project root, never the enclosing HOME"
        );
    }

    /// Nested registered projects confine to the innermost one.
    #[test]
    fn nested_projects_resolve_to_the_deepest() {
        let outer = tempfile::tempdir().unwrap();
        let inner = outer.path().join("inner");
        let leaf = inner.join("src");
        std::fs::create_dir_all(&leaf).unwrap();
        let registry = outer.path().join("registry");
        write_registry(&registry, "outer", outer.path());
        write_registry(&registry, "inner", &inner);

        let authority = resolve_workspace_authority(Some(&registry), &leaf).expect("covered");
        assert_eq!(authority.root(), inner.canonicalize().unwrap());
    }

    /// No registry at all is a refusal, not an implicit grant.
    #[test]
    fn absent_registry_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_workspace_authority(None, dir.path()).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    /// The core fail-closed case: a directory no registered project covers.
    #[test]
    fn uncovered_cwd_fails_closed() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let registry = home.path().join("registry");
        write_registry(&registry, "project", &project);

        let err = resolve_workspace_authority(Some(&registry), outside.path()).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    /// Containment is component-wise, so a sibling with a shared string prefix
    /// does not inherit the project's authority.
    #[test]
    fn containment_is_component_wise_not_string_prefix() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("repo");
        let inside = project.join("src");
        std::fs::create_dir_all(&inside).unwrap();
        let sibling = home.path().join("repo-evil");
        std::fs::create_dir_all(&sibling).unwrap();
        let registry = home.path().join("registry");
        write_registry(&registry, "repo", &project);

        assert!(resolve_workspace_authority(Some(&registry), &inside).is_ok());

        let err = resolve_workspace_authority(Some(&registry), &sibling).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }

    /// A registry entry pointing at a directory that does not exist is an
    /// invalid trusted root, not a silent skip — the operator asked for a
    /// project that is gone, and that is worth surfacing distinctly.
    #[test]
    fn stale_registry_entry_is_an_invalid_root() {
        let home = tempfile::tempdir().unwrap();
        let registry = home.path().join("registry");
        write_registry(&registry, "gone", &home.path().join("deleted"));

        let err = resolve_workspace_authority(Some(&registry), home.path()).unwrap_err();
        assert!(matches!(err, AuthorityError::NoTrustedContext { .. }));
    }
}
