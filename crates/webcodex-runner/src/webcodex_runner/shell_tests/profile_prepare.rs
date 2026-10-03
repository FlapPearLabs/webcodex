use super::*;

fn shell_with_default_profile() -> ShellConfig {
    let mut shell = ShellConfig::default();
    shell.default_profile = Some("project-profile".to_string());
    shell.profiles.insert(
        "project-profile".to_string(),
        ShellProfileConfig {
            init_script: Some("export PROFILE_TEST_VALUE=ready".to_string()),
            ..ShellProfileConfig::default()
        },
    );
    shell
}

fn seed_profile_cache(
    cache: &PreparedShellProfileCache,
    generation: u64,
    scope: ProfilePrepareScopeKind,
    project_key: String,
) {
    let key = PreparedShellProfileKey {
        generation,
        scope,
        project_key,
        profile_name: "project-profile".to_string(),
    };
    cache.profiles.lock().unwrap().insert(
        key,
        Arc::new(PreparedShellProfile {
            profile_name: "project-profile".to_string(),
            program: "/bin/sh".to_string(),
            args: Vec::new(),
            dialect: ShellDialect::Posix,
            env_snapshot: HashMap::from([("PROFILE_TEST_VALUE".to_string(), "cached".to_string())]),
        }),
    );
}

#[test]
fn default_profile_cache_hit_without_requested_cwd_still_requires_registry_authority() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = tempfile::tempdir().unwrap();
    let cache = PreparedShellProfileCache::default();
    seed_profile_cache(
        &cache,
        7,
        ProfilePrepareScopeKind::RegisteredWorkspace,
        shell_profile_project_key(None, workspace.path()),
    );

    let result = resolve_prepared_shell_profile(
        7,
        &shell_with_default_profile(),
        registry.path(),
        workspace.path(),
        false,
        &cache,
        None,
    );

    assert!(
        result.is_err(),
        "a cached cwd snapshot must not authorize profile preparation after project selection was skipped"
    );
}

#[test]
fn cached_registered_profile_is_refused_after_its_project_is_unregistered() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = tempfile::tempdir().unwrap();
    let registry_file = registry.path().join("project.toml");
    std::fs::write(
        &registry_file,
        format!(
            "id = \"project-id\"\npath = {:?}\n",
            workspace.path().to_string_lossy()
        ),
    )
    .unwrap();
    let cache = PreparedShellProfileCache::default();
    let project_key = shell_profile_project_key(Some("project-id"), workspace.path());
    seed_profile_cache(
        &cache,
        7,
        ProfilePrepareScopeKind::RegisteredWorkspace,
        project_key.clone(),
    );
    std::fs::remove_file(registry_file).unwrap();

    let result = cache.get_or_prepare(
        7,
        &shell_with_default_profile(),
        "project-profile",
        project_key,
        ProfilePrepareScope::RegisteredWorkspace {
            registry_dir: registry.path(),
        },
        workspace.path(),
        None,
    );

    assert!(
        result.is_err(),
        "a prepared snapshot must not survive removal of its registered authority"
    );
}

#[test]
fn provider_and_registered_profile_cache_scopes_do_not_share_entries() {
    let key = |scope| PreparedShellProfileKey {
        generation: 7,
        scope,
        project_key: "same-identity-for-regression-test".to_string(),
        profile_name: "project-profile".to_string(),
    };

    assert_ne!(
        key(ProfilePrepareScopeKind::RegisteredWorkspace),
        key(ProfilePrepareScopeKind::TrustedProvider)
    );
}

#[cfg(unix)]
#[test]
fn registered_profile_executable_resolution_keeps_symlink_alias_path() {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir().unwrap();
    let bin = workspace.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let alias = bin.join("configured-sh");
    symlink("/bin/sh", &alias).unwrap();
    let env = HashMap::from([("PATH".to_string(), bin.to_string_lossy().into_owned())]);

    let resolved =
        resolve_profile_prepare_program("configured-sh", &env, workspace.path()).unwrap();

    assert_eq!(resolved, alias);
    assert!(resolved.is_absolute());
}

#[test]
fn registered_profile_prepare_uses_broker_child_and_provider_keeps_its_explicit_raw_scope() {
    let source = include_str!("../shell.rs");
    let capture = source
        .split_once("fn capture_profile_env_snapshot(")
        .expect("profile snapshot capture function")
        .1
        .split_once("impl PreparedShellProfileCache")
        .expect("capture function ends before cache implementation")
        .0;
    let registered_prepare = capture
        .split_once("ProfilePrepareScope::RegisteredWorkspace { registry_dir } => {")
        .expect("registered workspace prepare branch")
        .1
        .split_once("    };")
        .expect("registered prepare branch ends before scope match closes")
        .0;

    assert!(registered_prepare.contains("spawn_local_action(Some(registry_dir), request)"));
    assert!(registered_prepare.contains("snapshot_env(&initial_env)"));
    assert!(!registered_prepare.contains("run_prepare_command("));
    assert!(capture.contains("ProfilePrepareScope::TrustedProvider =>"));
    assert!(capture.contains("run_prepare_command("));
}
